use crate::config::ModelId;
use crate::config::ToolDocumentationMode;
use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::tool_policy::ToolPolicy;
use crate::tools::mcp::MCP_QUALIFIED_TOOL_PREFIX;
use crate::tools::registry::{ToolHandler as RegistryToolHandler, ToolRegistration};
use crate::tools::tool_intent::ToolSurfaceKind;
use serde::Serialize;
use serde_json::{Value, json};

use super::tool_handler::{
    AdditionalProperties, ConfiguredToolSpec, JsonSchema, ResponsesApiTool, ToolSpec,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSurface {
    Interactive,
    AgentRunner,
    Acp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolModelCapabilities {
    pub supports_apply_patch_tool: bool,
}

impl ToolModelCapabilities {
    #[must_use]
    pub fn for_model_name(model_name: &str) -> Self {
        model_name
            .parse::<ModelId>()
            .ok()
            .map(|model_id| Self {
                supports_apply_patch_tool: model_id.supports_apply_patch_tool(),
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionToolsConfig {
    pub surface: SessionSurface,
    pub capability_level: CapabilityLevel,
    pub documentation_mode: ToolDocumentationMode,
    pub plan_mode: bool,
    pub request_user_input_enabled: bool,
    pub model_capabilities: ToolModelCapabilities,
}

impl SessionToolsConfig {
    pub fn full_public(
        surface: SessionSurface,
        capability_level: CapabilityLevel,
        documentation_mode: ToolDocumentationMode,
        model_capabilities: ToolModelCapabilities,
    ) -> Self {
        Self {
            surface,
            capability_level,
            documentation_mode,
            plan_mode: true,
            request_user_input_enabled: true,
            model_capabilities,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCatalogSource {
    Builtin,
    Mcp,
    Dynamic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogToolKind {
    Function,
    ApplyPatch,
}

#[derive(Debug, Clone)]
pub struct ToolCatalogEntry {
    pub public_name: String,
    pub registration_name: String,
    pub description: String,
    pub parameters: Value,
    pub aliases: Vec<String>,
    pub capability: CapabilityLevel,
    pub default_permission: ToolPolicy,
    pub supports_parallel_tool_calls: bool,
    pub source: ToolCatalogSource,
    pub kind: CatalogToolKind,
    pub configured_spec: ConfiguredToolSpec,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolSchemaEntry {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Default)]
pub struct SessionToolCatalog {
    entries: Vec<ToolCatalogEntry>,
}

impl SessionToolCatalog {
    pub fn new(entries: Vec<ToolCatalogEntry>) -> Self {
        Self { entries }
    }

    pub fn rebuild_from_registrations(registrations: Vec<ToolRegistration>) -> Self {
        let mut entries = Vec::new();
        for registration in registrations {
            if let Some(entry) = ToolCatalogEntry::from_registration(&registration) {
                entries.push(entry);
            }
        }

        entries.sort_by(|left, right| left.public_name.cmp(&right.public_name));
        entries.dedup_by(|left, right| left.public_name == right.public_name);
        Self { entries }
    }

    pub fn public_tool_names(&self, config: SessionToolsConfig) -> Vec<String> {
        self.filtered_entries(config)
            .map(|entry| entry.public_name.clone())
            .collect()
    }

    pub fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry> {
        self.filtered_entries(config)
            .map(|entry| ToolSchemaEntry {
                name: entry.public_name.clone(),
                description: compact_tool_description(
                    entry.description.as_str(),
                    config.documentation_mode,
                ),
                parameters: compact_parameters(entry.parameters.clone(), config.documentation_mode),
            })
            .collect()
    }

    pub fn function_declarations(&self, config: SessionToolsConfig) -> Vec<FunctionDeclaration> {
        self.schema_entries(config)
            .into_iter()
            .map(|entry| FunctionDeclaration {
                name: entry.name,
                description: entry.description,
                parameters: entry.parameters,
            })
            .collect()
    }

    pub fn model_tools(&self, config: SessionToolsConfig) -> Vec<ToolDefinition> {
        let mut tools = Vec::new();

        for entry in self.filtered_entries(config) {
            match entry.kind {
                CatalogToolKind::ApplyPatch
                    if config.model_capabilities.supports_apply_patch_tool =>
                {
                    tools.push(ToolDefinition::apply_patch(compact_tool_description(
                        entry.description.as_str(),
                        config.documentation_mode,
                    )));
                }
                _ => {
                    tools.push(ToolDefinition::function(
                        entry.public_name.clone(),
                        compact_tool_description(
                            entry.description.as_str(),
                            config.documentation_mode,
                        ),
                        compact_parameters(entry.parameters.clone(), config.documentation_mode),
                    ));
                }
            }
        }

        crate::prompts::sort_tool_definitions(tools)
    }

    pub fn schema_for_name(
        &self,
        name: &str,
        config: SessionToolsConfig,
    ) -> Option<ToolSchemaEntry> {
        self.schema_entries(config)
            .into_iter()
            .find(|entry| entry.name == name)
    }

    pub(crate) fn entries(&self) -> &[ToolCatalogEntry] {
        &self.entries
    }

    fn filtered_entries(
        &self,
        config: SessionToolsConfig,
    ) -> impl Iterator<Item = &ToolCatalogEntry> {
        self.entries
            .iter()
            .filter(move |entry| entry.is_visible(config))
    }
}

impl ToolCatalogEntry {
    fn from_registration(registration: &ToolRegistration) -> Option<Self> {
        let metadata = registration.metadata();
        let description = metadata.description()?.to_string();
        let parameters = metadata
            .parameter_schema()
            .cloned()
            .unwrap_or_else(default_parameter_schema);
        let default_permission = metadata.default_permission().unwrap_or(ToolPolicy::Prompt);
        let supports_parallel_tool_calls = registration_supports_parallel_tool_calls(registration);
        let aliases = metadata.aliases().to_vec();
        let kind = registration_catalog_kind(registration);

        if matches!(kind, CatalogToolKind::ApplyPatch) {
            let public_name = tools::APPLY_PATCH.to_string();
            return Some(Self::new(
                public_name,
                registration.name().to_string(),
                description,
                parameters,
                aliases,
                registration.capability(),
                default_permission,
                supports_parallel_tool_calls,
                ToolCatalogSource::Builtin,
                kind,
            ));
        }

        if registration.name().starts_with("mcp::") {
            let public_name = aliases
                .iter()
                .find(|alias| alias.starts_with(MCP_QUALIFIED_TOOL_PREFIX))
                .cloned()
                .or_else(|| aliases.first().cloned())?;
            return Some(Self::new(
                public_name,
                registration.name().to_string(),
                description,
                parameters,
                aliases,
                registration.capability(),
                default_permission,
                supports_parallel_tool_calls,
                ToolCatalogSource::Mcp,
                kind,
            ));
        }

        if !registration.expose_in_llm() {
            return None;
        }

        Some(Self::new(
            registration.name().to_string(),
            registration.name().to_string(),
            description,
            parameters,
            aliases,
            registration.capability(),
            default_permission,
            supports_parallel_tool_calls,
            ToolCatalogSource::Builtin,
            kind,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        public_name: String,
        registration_name: String,
        description: String,
        parameters: Value,
        aliases: Vec<String>,
        capability: CapabilityLevel,
        default_permission: ToolPolicy,
        supports_parallel_tool_calls: bool,
        source: ToolCatalogSource,
        kind: CatalogToolKind,
    ) -> Self {
        let configured_spec = ConfiguredToolSpec::new(
            ToolSpec::Function(ResponsesApiTool {
                name: public_name.clone(),
                description: description.clone(),
                strict: false,
                parameters: json_schema_from_value(&parameters),
            }),
            supports_parallel_tool_calls,
        );

        Self {
            public_name,
            registration_name,
            description,
            parameters,
            aliases,
            capability,
            default_permission,
            supports_parallel_tool_calls,
            source,
            kind,
            configured_spec,
        }
    }

    fn is_visible(&self, config: SessionToolsConfig) -> bool {
        if self.capability > config.capability_level {
            return false;
        }

        if !surface_allows_tool(config.surface, self.public_name.as_str()) {
            return false;
        }

        match self.public_name.as_str() {
            tools::REQUEST_USER_INPUT => config.request_user_input_enabled,
            tools::PLAN_TASK_TRACKER => config.plan_mode,
            _ => true,
        }
    }
}

fn surface_allows_tool(surface: SessionSurface, tool_name: &str) -> bool {
    match surface {
        SessionSurface::Interactive | SessionSurface::AgentRunner => true,
        SessionSurface::Acp => matches!(
            tool_name,
            tools::UNIFIED_SEARCH | tools::UNIFIED_FILE | tools::UNIFIED_EXEC
        ),
    }
}

fn registration_catalog_kind(registration: &ToolRegistration) -> CatalogToolKind {
    registration
        .metadata()
        .behavior()
        .map(|behavior| match behavior.surface_kind {
            ToolSurfaceKind::Function => CatalogToolKind::Function,
            ToolSurfaceKind::ApplyPatch => CatalogToolKind::ApplyPatch,
        })
        .unwrap_or(CatalogToolKind::Function)
}

fn registration_supports_parallel_tool_calls(registration: &ToolRegistration) -> bool {
    if let Some(behavior) = registration.metadata().behavior() {
        return behavior.supports_parallel_calls;
    }

    match registration.handler() {
        RegistryToolHandler::TraitObject(tool) => tool.is_parallel_safe(),
        RegistryToolHandler::RegistryFn(_) => false,
    }
}

pub(crate) fn unified_exec_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {
                "description": "Command as a shell string or argv array.",
                "anyOf": [
                    {"type": "string"},
                    {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                ]
            },
            "input": {"type": "string", "description": "stdin content for write action."},
            "session_id": {"type": "string", "description": "Session id for write/poll/continue/inspect/close."},
            "spool_path": {"type": "string", "description": "Spool file path for inspect action."},
            "query": {"type": "string", "description": "Optional line filter for inspect output or run output."},
            "head_lines": {"type": "integer", "description": "Inspect head preview lines."},
            "tail_lines": {"type": "integer", "description": "Inspect tail preview lines."},
            "max_matches": {"type": "integer", "description": "Max filtered matches for inspect or run query.", "default": 200},
            "literal": {"type": "boolean", "description": "Treat query as literal text.", "default": false},
            "code": {"type": "string", "description": "Code to execute for code action."},
            "language": {
                "type": "string",
                "enum": ["python3", "javascript"],
                "description": "Language for code action.",
                "default": "python3"
            },
            "action": {
                "type": "string",
                "enum": ["run", "write", "poll", "continue", "inspect", "list", "close", "code"],
                "description": "Action. Inferred from command/code/input/session_id/spool_path when omitted."
            },
            "workdir": {"type": "string", "description": "Working directory for new sessions."},
            "cwd": {"type": "string", "description": "Alias for workdir."},
            "tty": {"type": "boolean", "description": "Run the command in a PTY instead of pipe mode.", "default": false},
            "shell": {"type": "string", "description": "Shell binary."},
            "login": {"type": "boolean", "description": "Use login shell.", "default": false},
            "sandbox_permissions": {
                "type": "string",
                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                "description": "Sandbox permissions for the command. Use `with_additional_permissions` to request extra sandboxed filesystem access, or `require_escalated` to run without sandbox restrictions."
            },
            "additional_permissions": {
                "type": "object",
                "description": "Only used with `sandbox_permissions=with_additional_permissions`.",
                "properties": {
                    "fs_read": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Additional filesystem paths to grant read access."
                    },
                    "fs_write": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Additional filesystem paths to grant write access."
                    }
                },
                "additionalProperties": false
            },
            "justification": {"type": "string", "description": "Approval question shown to the user when requesting `require_escalated` execution. Required when `sandbox_permissions=require_escalated`."},
            "prefix_rule": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Optional command prefix to persist when the user chooses permanent approval. Must be a prefix of `command`, and VT Code ignores it for compound shell commands."
            },
            "timeout_secs": {"type": "integer", "description": "Timeout in seconds.", "default": 180},
            "yield_time_ms": {"type": "integer", "description": "Time to wait for output (ms).", "default": 1000},
            "confirm": {"type": "boolean", "description": "Confirm destructive ops.", "default": false},
            "max_output_tokens": {"type": "integer", "description": "Max output tokens."},
            "track_files": {"type": "boolean", "description": "Track file changes during code execution.", "default": false}
        }
    })
}

pub(crate) fn unified_file_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["read", "write", "edit", "patch", "delete", "move", "copy"],
                "description": "Action to perform. If not provided, inferred: 'edit' if old_str present, 'patch' if patch/input patch content present, 'write' if content present, 'move' if destination present, 'read' if a path key is present."
            },
            "path": {"type": "string", "description": "File path (relative to workspace root)."},
            "content": {"type": "string", "description": "New content for 'write' action."},
            "old_str": {"type": "string", "description": "EXACT text to replace for 'edit' action. Must match file content exactly including whitespace and newlines."},
            "new_str": {"type": "string", "description": "Replacement text for 'edit' action."},
            "patch": {"type": "string", "description": "Patch content for 'patch' action. Use '*** Update File: path' format with @@ hunks, NOT unified diff (---/+++ format)."},
            "destination": {"type": "string", "description": "Target path for 'move' or 'copy' actions."},
            "start_line": {"type": "integer", "description": "Start line for 'read' action (1-indexed)."},
            "end_line": {"type": "integer", "description": "End line for 'read' action (inclusive)."},
            "offset": {"type": "integer", "description": "Alias for start_line."},
            "limit": {"type": "integer", "description": "Number of lines to read."},
            "mode": {"type": "string", "description": "Mode for 'read' (e.g., 'head', 'tail') or 'write' (e.g., 'fail_if_exists')."},
            "indentation": {"type": "boolean", "description": "Include indentation info in 'read' output.", "default": false}
        }
    })
}

pub(crate) fn unified_search_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["grep", "list", "structural", "tools", "errors", "agent", "web", "skill"],
                "description": "Action to perform. Default to `structural` for code or syntax-aware search, `grep` for raw text, and `list` for file discovery. Refine and retry `grep` or `structural` here before switching tools."
            },
            "pattern": {"type": "string", "description": "For `grep` or `errors`, regex or literal text. For `structural`, valid parseable code for the selected language, not a raw code fragment; if a fragment fails, retry `action='structural'` with a larger parseable pattern such as a full function signature."},
            "path": {"type": "string", "description": "Directory or file path to search in.", "default": "."},
            "lang": {"type": "string", "description": "Language for structural search. Set it whenever the code language is known; required for debug_query."},
            "selector": {"type": "string", "description": "ast-grep selector for structural search when the real match is a subnode inside the parseable pattern."},
            "strictness": {
                "type": "string",
                "enum": ["cst", "smart", "ast", "relaxed", "signature", "template"],
                "description": "Pattern strictness for structural search."
            },
            "debug_query": {
                "type": "string",
                "enum": ["pattern", "ast", "cst", "sexp"],
                "description": "Print the structural query AST instead of matches. Requires lang."
            },
            "globs": {
                "description": "Optional include/exclude globs for structural search.",
                "anyOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "keyword": {"type": "string", "description": "Keyword for 'tools' search."},
            "url": {"type": "string", "format": "uri", "description": "The URL to fetch content from (for 'web' action)."},
            "prompt": {"type": "string", "description": "The prompt to run on the fetched content (for 'web' action)."},
            "name": {"type": "string", "description": "Skill name to load (for 'skill' action)."},
            "detail_level": {
                "type": "string",
                "enum": ["name-only", "name-and-description", "full"],
                "description": "Detail level for 'tools' action.",
                "default": "name-and-description"
            },
            "mode": {
                "type": "string",
                "description": "Mode for 'list' (list|recursive|tree|etc) or 'agent' (debug|analyze|full) action.",
                "default": "list"
            },
            "max_results": {"type": "integer", "description": "Max results to return.", "default": 100},
            "case_sensitive": {"type": "boolean", "description": "Case-sensitive search.", "default": false},
            "context_lines": {"type": "integer", "description": "Context lines for 'grep' or 'structural' results.", "default": 0},
            "scope": {"type": "string", "description": "Scope for 'errors' action (archive|all).", "default": "archive"},
            "max_bytes": {"type": "integer", "description": "Maximum bytes to fetch for 'web' action.", "default": 500000},
            "timeout_secs": {"type": "integer", "description": "Timeout in seconds.", "default": 30}
        }
    })
}

pub(crate) fn apply_patch_parameters() -> Value {
    crate::tools::apply_patch::parameter_schema(
        "Patch in VT Code format: *** Begin Patch, *** Update File: path, @@ hunk, -/+ lines, *** End Patch",
    )
}

fn default_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

fn compact_tool_description(original: &str, mode: ToolDocumentationMode) -> String {
    let max_len = match mode {
        ToolDocumentationMode::Minimal => 64,
        ToolDocumentationMode::Progressive => 120,
        ToolDocumentationMode::Full => usize::MAX,
    };

    let sentence = original
        .split('.')
        .next()
        .unwrap_or(original)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if sentence.len() <= max_len {
        sentence
    } else {
        let target = max_len.saturating_sub(1);
        let end = sentence
            .char_indices()
            .map(|(index, _)| index)
            .rfind(|&index| index <= target)
            .unwrap_or(0);
        format!("{}…", &sentence[..end])
    }
}

fn compact_parameters(parameters: Value, mode: ToolDocumentationMode) -> Value {
    if matches!(mode, ToolDocumentationMode::Full) {
        return parameters;
    }

    let mut compacted = parameters;
    remove_schema_descriptions(&mut compacted);
    compacted
}

fn remove_schema_descriptions(value: &mut Value) {
    remove_schema_descriptions_impl(value, false);
}

fn remove_schema_descriptions_impl(value: &mut Value, inside_properties_map: bool) {
    match value {
        Value::Object(map) => {
            if !inside_properties_map {
                map.remove("description");
            }
            for (key, nested) in map.iter_mut() {
                remove_schema_descriptions_impl(nested, key == "properties");
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_schema_descriptions_impl(item, false);
            }
        }
        _ => {}
    }
}

fn json_schema_from_value(value: &Value) -> JsonSchema {
    match value {
        Value::Object(map) => match map.get("type").and_then(Value::as_str) {
            Some("object") => {
                let properties = map
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|props| {
                        props
                            .iter()
                            .map(|(key, value)| (key.clone(), json_schema_from_value(value)))
                            .collect()
                    })
                    .unwrap_or_default();
                let required = map.get("required").and_then(Value::as_array).map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()
                });
                let additional_properties =
                    map.get("additionalProperties").map(|value| match value {
                        Value::Bool(flag) => AdditionalProperties::Boolean(*flag),
                        Value::Object(_) => {
                            AdditionalProperties::Schema(Box::new(json_schema_from_value(value)))
                        }
                        _ => AdditionalProperties::Boolean(true),
                    });
                let any_of = map.get("anyOf").and_then(Value::as_array).cloned();

                JsonSchema::Object {
                    properties,
                    required,
                    additional_properties,
                    any_of,
                }
            }
            Some("array") => JsonSchema::Array {
                items: Box::new(
                    map.get("items")
                        .map(json_schema_from_value)
                        .unwrap_or(JsonSchema::Null),
                ),
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            Some("boolean") => JsonSchema::Boolean {
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            Some("integer" | "number") => JsonSchema::Number {
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            Some("string") => JsonSchema::String {
                description: map
                    .get("description")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            },
            _ => {
                if map.contains_key("enum") {
                    JsonSchema::String {
                        description: map
                            .get("description")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    }
                } else {
                    JsonSchema::Null
                }
            }
        },
        _ => JsonSchema::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::registry::ToolRegistration;
    use crate::tools::request_user_input::RequestUserInputTool;
    use crate::tools::tool_intent::{ToolBehavior, ToolMutationModel};
    use crate::tools::traits::Tool;
    use serde_json::json;

    fn registration(name: &'static str) -> ToolRegistration {
        ToolRegistration::new(name, CapabilityLevel::CodeSearch, false, |_, _| {
            Box::pin(async { Ok(Value::Null) })
        })
    }

    #[test]
    fn rebuild_catalog_uses_public_mcp_alias() {
        let registration = registration("mcp::context7::search")
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(json!({"type":"object"}))
            .with_aliases(["mcp__context7__search"]);

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let names = catalog.public_tool_names(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ));

        assert_eq!(names, vec!["mcp__context7__search".to_string()]);
    }

    #[test]
    fn schema_entries_hide_request_user_input_when_disabled() {
        let registration = registration(tools::REQUEST_USER_INPUT)
            .with_description("Ask the user")
            .with_parameter_schema(json!({"type":"object"}));

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let names = catalog.public_tool_names(SessionToolsConfig {
            surface: SessionSurface::Interactive,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: ToolDocumentationMode::Full,
            plan_mode: true,
            request_user_input_enabled: false,
            model_capabilities: ToolModelCapabilities::default(),
        });

        assert!(names.is_empty());
    }

    #[test]
    fn apply_patch_uses_special_tool_when_supported() {
        let registration = registration(tools::APPLY_PATCH)
            .with_llm_visibility(false)
            .with_description("Apply patch")
            .with_parameter_schema(apply_patch_parameters())
            .with_behavior(ToolBehavior::apply_patch(
                ToolMutationModel::Mutating,
                false,
                true,
            ));

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let tools = catalog.model_tools(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities {
                supports_apply_patch_tool: true,
            },
        ));

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_type, "apply_patch");
    }

    #[test]
    fn apply_patch_falls_back_to_function_tool_when_unsupported() {
        let registration = registration(tools::APPLY_PATCH)
            .with_llm_visibility(false)
            .with_description("Apply patch")
            .with_parameter_schema(apply_patch_parameters())
            .with_behavior(ToolBehavior::apply_patch(
                ToolMutationModel::Mutating,
                false,
                true,
            ));

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let tools = catalog.model_tools(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ));

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_type, "function");
    }

    #[test]
    fn unified_exec_schema_accepts_string_or_array_commands() {
        let params = unified_exec_parameters();
        let command = &params["properties"]["command"];
        let variants = command["anyOf"].as_array().expect("command anyOf");

        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["type"], "string");
        assert_eq!(variants[1]["type"], "array");
        assert_eq!(variants[1]["items"]["type"], "string");
        assert_eq!(params["properties"]["tty"]["type"], "boolean");
        assert_eq!(params["properties"]["tty"]["default"], false);
    }

    #[test]
    fn unified_search_schema_advertises_structural_and_hides_intelligence() {
        let params = unified_search_parameters();
        let actions = params["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");

        assert!(actions.iter().any(|value| value == "structural"));
        assert!(!actions.iter().any(|value| value == "intelligence"));
        assert!(
            params["properties"]["debug_query"]["enum"]
                .as_array()
                .expect("debug_query enum")
                .iter()
                .any(|value| value == "ast")
        );
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("Default to `structural`"),
            "schema should bias code search toward structural mode"
        );
        assert!(
            params["properties"]["pattern"]["description"]
                .as_str()
                .expect("pattern description")
                .contains("valid parseable code"),
            "schema should explain structural pattern requirements"
        );
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("Refine and retry `grep` or `structural`"),
            "schema should keep search refinement inside unified_search"
        );
    }

    #[test]
    fn parallel_support_comes_from_behavior_metadata() {
        let registration = registration("parallel_catalog_tool")
            .with_description("parallel-safe test tool")
            .with_parameter_schema(json!({"type":"object"}))
            .with_behavior(ToolBehavior::function(
                ToolMutationModel::ReadOnly,
                true,
                false,
            ));

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        assert_eq!(catalog.entries().len(), 1);
        assert!(catalog.entries()[0].supports_parallel_tool_calls);
    }

    #[test]
    fn configured_spec_preserves_json_schema_field_names() {
        let registration = registration("schema_contract_tool")
            .with_description("schema contract")
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                },
                "additionalProperties": false,
                "anyOf": [
                    {"required": ["input"]},
                    {"required": ["patch"]}
                ]
            }));

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let entry = &catalog.entries()[0];
        let ToolSpec::Function(tool) = &entry.configured_spec.spec else {
            panic!("expected function tool spec");
        };

        let serialized = serde_json::to_value(&tool.parameters).expect("serialize parameters");
        assert_eq!(serialized["additionalProperties"], Value::Bool(false));
        assert!(serialized["anyOf"].is_array());
        assert!(serialized.get("additional_properties").is_none());
        assert!(serialized.get("any_of").is_none());
    }

    #[test]
    fn compact_parameters_preserves_property_named_description() {
        let schema = RequestUserInputTool
            .parameter_schema()
            .expect("request_user_input schema");

        let compacted = compact_parameters(schema, ToolDocumentationMode::Progressive);
        let description_property = &compacted["properties"]["questions"]["items"]["properties"]["options"]
            ["items"]["properties"]["description"];

        assert!(description_property.is_object());
        assert_eq!(
            compacted["properties"]["questions"]["items"]["properties"]["options"]["items"]["required"],
            json!(["label", "description"])
        );
    }
}
