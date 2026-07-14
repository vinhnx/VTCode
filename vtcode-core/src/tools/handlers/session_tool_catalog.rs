use crate::config::ModelId;
use crate::config::ToolDocumentationMode;
use crate::config::constants::tools;
use crate::config::loader::VTCodeConfig;
use crate::config::models::Provider;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::{ToolDefinition, ToolNamespace, ToolSearchAlgorithm};
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::tool_policy::ToolPolicy;
use crate::tools::mcp::MCP_QUALIFIED_TOOL_PREFIX;
use crate::tools::registry::{ToolHandler as RegistryToolHandler, ToolRegistration};
use crate::tools::tool_intent::ToolSurfaceKind;
use crate::utils::tool_name_parsing::parse_canonical_mcp_tool_name;
use rustc_hash::FxHashSet;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use vtcode_utility_tool_specs::parse_tool_input_schema;

use super::tool_handler::{ConfiguredToolSpec, ResponsesApiTool, ToolSpec};

pub use crate::config::ToolProfile;
pub use crate::tools::registry::ToolCatalogSource;

/// The surface (execution context) where tools are exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSurface {
    /// Interactive TUI session.
    Interactive,
    /// Non-interactive agent runner.
    AgentRunner,
    /// Agent Client Protocol (ACP) session.
    Acp,
}

/// Model-specific capabilities that affect tool catalog generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolModelCapabilities {
    /// Whether the model supports the native `apply_patch` tool.
    pub supports_apply_patch_tool: bool,
}

impl ToolModelCapabilities {
    /// Returns capabilities inferred from the model name.
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

/// The kind of deferred tool search supported by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredToolSearchKind {
    /// Anthropic's tool search with a specific algorithm.
    Anthropic(ToolSearchAlgorithm),
    /// OpenAI's hosted tool search.
    OpenAIHosted,
    /// Client-local MCP tool search for providers with no hosted tool search.
    /// `mcp_search_tools` remains available while matched MCP definitions are
    /// expanded into the next request. Deferred definitions are omitted from
    /// the current wire payload rather than sent with `defer_loading: true`.
    ClientLocal,
}

/// Above this many deferable (non-core, non-`always_available`) tools, a
/// catalog is exposed via deferred loading rather than sent eagerly. Below it,
/// eager exposure is cheaper and simpler. Ignored when the catalog contains any
/// MCP tool (see `model_tools`), since MCP schemas are the dominant token cost.
const DIRECT_TOOL_EXPOSURE_THRESHOLD: usize = 15;
/// Token budget (~4 chars/token) for the combined schema of a deferable
/// catalog. A catalog is deferred when its estimated schema size exceeds this,
/// even if the tool count is below `DIRECT_TOOL_EXPOSURE_THRESHOLD`. This catches
/// a single large server whose schema dwarfs the entire builtin set.
const DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET: usize = 4_000;

/// Policy for deferred tool loading (tool search).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeferredToolPolicy {
    search_kind: Option<DeferredToolSearchKind>,
    always_available_tools: BTreeSet<String>,
}

impl DeferredToolPolicy {
    /// Creates a policy for Anthropic's tool search.
    #[must_use]
    pub fn anthropic(
        algorithm: ToolSearchAlgorithm,
        always_available_tools: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            search_kind: Some(DeferredToolSearchKind::Anthropic(algorithm)),
            always_available_tools: always_available_tools.into_iter().collect(),
        }
    }

    /// Creates a policy for OpenAI's hosted tool search.
    #[must_use]
    pub fn openai_hosted(always_available_tools: impl IntoIterator<Item = String>) -> Self {
        Self {
            search_kind: Some(DeferredToolSearchKind::OpenAIHosted),
            always_available_tools: always_available_tools.into_iter().collect(),
        }
    }

    /// Creates a policy for client-local tool search. Used for providers
    /// with no hosted tool search when `client_tool_search` is enabled.
    #[must_use]
    pub fn client_local(always_available_tools: impl IntoIterator<Item = String>) -> Self {
        Self {
            search_kind: Some(DeferredToolSearchKind::ClientLocal),
            always_available_tools: always_available_tools.into_iter().collect(),
        }
    }

    fn is_enabled(&self) -> bool {
        self.search_kind.is_some()
    }

    /// Returns whether this policy defers via client-local tool search
    /// rather than a provider-hosted mechanism. Callers that assemble the
    /// wire-level request tool list use this to decide whether deferred
    /// tool definitions may be omitted from the payload -- hosted policies
    /// (Anthropic/OpenAI) require the full deferred definitions to remain
    /// on the wire, so this must stay `false` for those.
    #[must_use]
    pub fn is_client_local(&self) -> bool {
        matches!(self.search_kind, Some(DeferredToolSearchKind::ClientLocal))
    }

    fn keeps_entry_available(&self, entry: &ToolCatalogEntry) -> bool {
        self.always_available_tools
            .contains(entry.public_name.as_str())
            || self
                .always_available_tools
                .contains(entry.registration_name.as_str())
            || entry
                .aliases
                .iter()
                .any(|alias| self.always_available_tools.contains(alias.as_str()))
    }

    fn tool_search_definition(&self) -> Option<ToolDefinition> {
        match self.search_kind {
            Some(DeferredToolSearchKind::Anthropic(algorithm)) => {
                Some(ToolDefinition::tool_search(algorithm))
            }
            Some(DeferredToolSearchKind::OpenAIHosted) => {
                Some(ToolDefinition::hosted_tool_search())
            }
            // `unified_search` is a core tool and always present; there is
            // no separate wire-level tool to inject for client-local
            // deferral.
            Some(DeferredToolSearchKind::ClientLocal) | None => None,
        }
    }
}

/// Returns the deferred tool policy for the given provider and configuration.
#[must_use]
pub fn deferred_tool_policy_for_runtime(
    provider: Option<Provider>,
    model_supports_responses_compaction: bool,
    vtcode_config: Option<&VTCodeConfig>,
) -> DeferredToolPolicy {
    match provider {
        Some(Provider::Anthropic) => {
            let enabled =
                vtcode_config.is_none_or(|cfg| cfg.provider.anthropic.tool_search.enabled);
            let defer_by_default =
                vtcode_config.is_none_or(|cfg| cfg.provider.anthropic.tool_search.defer_by_default);
            if !enabled || !defer_by_default {
                return DeferredToolPolicy::default();
            }

            let algorithm = vtcode_config
                .map(|cfg| cfg.provider.anthropic.tool_search.algorithm)
                .unwrap_or_default();
            let always_available_tools = vtcode_config
                .map(|cfg| {
                    cfg.provider
                        .anthropic
                        .tool_search
                        .always_available_tools
                        .clone()
                })
                .unwrap_or_default();
            DeferredToolPolicy::anthropic(algorithm, always_available_tools)
        }
        Some(Provider::OpenAI) if model_supports_responses_compaction => {
            let enabled = vtcode_config.is_none_or(|cfg| cfg.provider.openai.tool_search.enabled);
            let defer_by_default =
                vtcode_config.is_none_or(|cfg| cfg.provider.openai.tool_search.defer_by_default);
            if !enabled || !defer_by_default {
                return DeferredToolPolicy::default();
            }

            let always_available_tools = vtcode_config
                .map(|cfg| {
                    cfg.provider
                        .openai
                        .tool_search
                        .always_available_tools
                        .clone()
                })
                .unwrap_or_default();
            DeferredToolPolicy::openai_hosted(always_available_tools)
        }
        _ => {
            // No provider-hosted tool search is available (e.g. Gemini).
            // Client-local deferral is now the default so MCP schemas are not
            // sent eagerly. Users can opt back to the eager catalog by setting
            // `tools.client_tool_search = false`. The `DIRECT_TOOL_EXPOSURE_THRESHOLD`
            // and `DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET` gating that decides whether
            // deferral is actually worthwhile for a given catalog lives downstream
            // in `SessionToolCatalog::model_tools`, exactly as it does for the
            // hosted arms above -- this function only decides whether deferral is
            // *possible* for the runtime, not whether it is *used* for the
            // current catalog.
            let client_tool_search_enabled =
                vtcode_config.is_some_and(|cfg| cfg.tools.client_tool_search);
            if client_tool_search_enabled {
                DeferredToolPolicy::client_local(Vec::new())
            } else {
                DeferredToolPolicy::default()
            }
        }
    }
}

/// Returns whether Anthropic native memory is enabled for the given runtime.
#[must_use]
pub fn anthropic_native_memory_enabled_for_runtime(
    provider: Option<Provider>,
    model: &str,
    vtcode_config: Option<&VTCodeConfig>,
) -> bool {
    matches!(provider, Some(Provider::Anthropic))
        && !matches!(
            crate::llm::factory::infer_provider(None, model),
            Some(resolved) if resolved != Provider::Anthropic
        )
        && vtcode_config.is_some_and(|cfg| cfg.provider.anthropic.memory.enabled)
}

/// Configuration for the session's tool catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionToolsConfig {
    /// The execution surface (interactive, agent runner, ACP).
    pub surface: SessionSurface,
    /// Minimum capability level required for tools to be visible.
    pub capability_level: CapabilityLevel,
    /// Documentation detail mode for tool descriptions.
    pub documentation_mode: ToolDocumentationMode,
    /// Whether the planning workflow is active.
    pub planning_active: bool,
    /// Whether the request_user_input tool is enabled.
    pub request_user_input_enabled: bool,
    /// Model-specific capabilities.
    pub model_capabilities: ToolModelCapabilities,
    /// Policy for deferred tool loading.
    pub deferred_tool_policy: DeferredToolPolicy,
    /// Whether Anthropic native memory is enabled.
    pub anthropic_native_memory_enabled: bool,
    /// Model-facing tool profile.
    pub tool_profile: ToolProfile,
}

impl SessionToolsConfig {
    /// Creates a public configuration for a session outside the planning workflow.
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
            planning_active: false,
            request_user_input_enabled: true,
            model_capabilities,
            deferred_tool_policy: DeferredToolPolicy::default(),
            anthropic_native_memory_enabled: false,
            tool_profile: ToolProfile::CodexDefault,
        }
    }

    /// Marks whether the planning workflow is active.
    #[must_use]
    pub fn with_planning_active(mut self, planning_active: bool) -> Self {
        self.planning_active = planning_active;
        self
    }

    /// Sets the deferred tool policy.
    #[must_use]
    pub fn with_deferred_tool_policy(mut self, deferred_tool_policy: DeferredToolPolicy) -> Self {
        self.deferred_tool_policy = deferred_tool_policy;
        self
    }

    /// Enables or disables Anthropic native memory.
    #[must_use]
    pub fn with_anthropic_native_memory_enabled(mut self, enabled: bool) -> Self {
        self.anthropic_native_memory_enabled = enabled;
        self
    }

    /// Selects the model-facing tool profile.
    #[must_use]
    pub fn with_tool_profile(mut self, tool_profile: ToolProfile) -> Self {
        self.tool_profile = tool_profile;
        self
    }
}

/// The kind of tool in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogToolKind {
    /// Standard function call tool.
    Function,
    /// Native apply_patch tool.
    ApplyPatch,
}

/// An entry in the session tool catalog.
#[derive(Debug, Clone)]
pub struct ToolCatalogEntry {
    /// Name exposed to the LLM.
    pub public_name: String,
    /// Internal registration name.
    pub registration_name: String,
    /// Tool description.
    pub description: String,
    /// JSON Schema for tool parameters.
    pub parameters: Value,
    /// Alternative names for the tool.
    pub aliases: Vec<String>,
    /// Minimum capability level required to use this tool.
    pub capability: CapabilityLevel,
    /// Default permission policy for this tool.
    pub default_permission: ToolPolicy,
    /// Whether this tool supports parallel execution.
    pub supports_parallel_tool_calls: bool,
    /// Source of this tool in the catalog.
    pub source: ToolCatalogSource,
    /// The kind of tool (function or apply_patch).
    pub kind: CatalogToolKind,
    /// The configured tool specification.
    pub configured_spec: ConfiguredToolSpec,
    /// Optional per-tool description length cap. When set, overrides the
    /// documentation mode's default max length. Used for MCP tools whose
    /// descriptions can be arbitrarily long.
    pub max_description_length: Option<usize>,
    /// Namespace grouping derived from the registration (currently only MCP
    /// tools, keyed by server name). `None` for core/builtin tools.
    pub namespace: Option<ToolNamespace>,
}

/// A simplified tool schema entry for serialization.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolSchemaEntry {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON Schema for tool parameters.
    pub parameters: Value,
}

/// The session's tool catalog containing all available tools.
#[derive(Debug, Clone, Default)]
pub struct SessionToolCatalog {
    entries: Vec<ToolCatalogEntry>,
}

/// Estimate the visible tool-schema token count for deferral budgeting.
///
/// Uses a compacted representation matching what would be sent on the wire,
/// then divides by a conservative 4 characters-per-token ratio. This keeps
/// huge single-server MCP schemas from being sent eagerly even when their
/// tool count is below the numeric threshold.
fn estimate_schema_tokens(entries: &[&ToolCatalogEntry], config: &SessionToolsConfig) -> usize {
    entries
        .iter()
        .map(|entry| {
            let description = compact_tool_description(
                entry.description.as_str(),
                config.documentation_mode,
                entry.max_description_length,
            );
            let parameters =
                compact_parameters(entry.parameters.clone(), config.documentation_mode);
            let entry = ToolSchemaEntry {
                name: entry.public_name.clone(),
                description,
                parameters,
            };
            serde_json::to_string(&entry)
                .map(|s| s.len() / 4)
                .unwrap_or(0)
        })
        .sum()
}

impl SessionToolCatalog {
    /// Creates a new catalog from the given entries.
    pub fn new(entries: Vec<ToolCatalogEntry>) -> Self {
        Self { entries }
    }

    /// Rebuilds the catalog from tool registrations.
    pub fn rebuild_from_registrations(registrations: Vec<ToolRegistration>) -> Self {
        let mut entries = Vec::new();
        for registration in registrations {
            if let Some(entry) = ToolCatalogEntry::from_registration(&registration) {
                entries.push(entry);
            }
        }

        let mut seen_public_names = FxHashSet::default();
        entries.retain(|entry| seen_public_names.insert(entry.public_name.clone()));
        Self { entries }
    }

    /// Returns the names of all public tools visible with the given config.
    pub fn public_tool_names(&self, config: SessionToolsConfig) -> Vec<String> {
        self.filtered_entries(&config)
            .map(|entry| entry.public_name.clone())
            .collect()
    }

    /// Returns schema entries for all visible tools.
    pub fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry> {
        self.filtered_entries(&config)
            .map(|entry| ToolSchemaEntry {
                name: entry.public_name.clone(),
                description: compact_tool_description(
                    entry.description.as_str(),
                    config.documentation_mode,
                    entry.max_description_length,
                ),
                parameters: compact_parameters(entry.parameters.clone(), config.documentation_mode),
            })
            .collect()
    }

    /// Returns Gemini function declarations for all visible tools.
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

    /// Returns tool definitions for the LLM, including deferred loading support.
    pub fn model_tools(&self, config: SessionToolsConfig) -> Vec<ToolDefinition> {
        let filtered_entries = self.filtered_entries(&config).collect::<Vec<_>>();
        let deferable_tool_count = filtered_entries
            .iter()
            .filter(|entry| should_defer_tool_loading(entry, &config))
            .count();
        let estimated_schema_tokens = estimate_schema_tokens(&filtered_entries, &config);
        let has_mcp_tools = filtered_entries
            .iter()
            .any(|entry| matches!(entry.source, ToolCatalogSource::Mcp));
        let expose_tools_directly = !config.deferred_tool_policy.is_enabled()
            || (deferable_tool_count < DIRECT_TOOL_EXPOSURE_THRESHOLD
                && !has_mcp_tools
                && estimated_schema_tokens <= DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET);
        let mut tools = Vec::new();
        let mut has_deferred_tools = false;

        for entry in filtered_entries {
            let defer_loading = should_defer_tool_loading(entry, &config);
            match entry.kind {
                CatalogToolKind::ApplyPatch
                    if config.model_capabilities.supports_apply_patch_tool =>
                {
                    let mut tool = ToolDefinition::apply_patch(compact_tool_description(
                        entry.description.as_str(),
                        config.documentation_mode,
                        entry.max_description_length,
                    ));
                    if defer_loading && !expose_tools_directly {
                        tool = tool.with_defer_loading(true);
                        has_deferred_tools = true;
                    }
                    tools.push(tool);
                }
                _ => {
                    let mut tool = if entry.public_name == tools::MEMORY {
                        ToolDefinition::anthropic_memory()
                    } else {
                        ToolDefinition::function(
                            entry.public_name.clone(),
                            compact_tool_description(
                                entry.description.as_str(),
                                config.documentation_mode,
                                entry.max_description_length,
                            ),
                            compact_parameters(entry.parameters.clone(), config.documentation_mode),
                        )
                    };
                    if defer_loading && !expose_tools_directly {
                        tool = tool.with_defer_loading(true);
                        has_deferred_tools = true;
                        // Namespace metadata is only attached to deferred
                        // tools. It never reaches the wire payload (provider
                        // formatters build their JSON manually field-by-field
                        // for function tools and never serde-serialize the
                        // whole `ToolDefinition`), but restricting it to the
                        // deferred case keeps the blast radius small and
                        // matches the article's design: namespace grouping
                        // only matters once a tool is discoverable-only.
                        if let Some(namespace) = entry.namespace.clone() {
                            tool = tool.with_namespace(namespace);
                        }
                    }
                    tools.push(tool);
                }
            }
        }

        if has_deferred_tools
            && let Some(search_tool) = config.deferred_tool_policy.tool_search_definition()
        {
            tools.push(search_tool);
        }

        tools
    }

    /// Returns the schema entry for a tool by name.
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
        config: &SessionToolsConfig,
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
        let source = registration_catalog_source(registration, kind);

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
                source,
                kind,
            ));
        }

        if registration.name().starts_with("mcp::") {
            let public_name = aliases
                .iter()
                .find(|alias| alias.starts_with(MCP_QUALIFIED_TOOL_PREFIX))
                .cloned()
                .or_else(|| aliases.first().cloned())?;
            let mut entry = Self::new(
                public_name,
                registration.name().to_string(),
                description,
                parameters,
                aliases,
                registration.capability(),
                default_permission,
                supports_parallel_tool_calls,
                source,
                kind,
            );
            // MCP tool descriptions from external servers can be arbitrarily
            // long. Cap them to prevent token inflation.
            entry.max_description_length = Some(MCP_TOOL_DESCRIPTION_MAX_LEN);
            if let Some((server, _tool)) = parse_canonical_mcp_tool_name(registration.name()) {
                let namespace_description = metadata
                    .server_hint()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("Tools provided by MCP server '{server}'"));
                entry.namespace = Some(ToolNamespace {
                    name: server.to_string(),
                    description: namespace_description,
                });
            }
            return Some(entry);
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
            source,
            kind,
        ))
    }

    #[expect(clippy::too_many_arguments)]
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
                parameters: parse_tool_input_schema(&parameters),
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
            max_description_length: None,
            namespace: None,
        }
    }

    fn is_visible(&self, config: &SessionToolsConfig) -> bool {
        if self.capability > config.capability_level {
            return false;
        }

        if !profile_allows_tool(
            config.tool_profile,
            self.public_name.as_str(),
            config.planning_active,
        ) {
            return false;
        }

        if !surface_allows_tool(config.surface, self.public_name.as_str()) {
            return false;
        }

        match self.public_name.as_str() {
            tools::MEMORY => config.anthropic_native_memory_enabled,
            tools::REQUEST_USER_INPUT => config.request_user_input_enabled,
            _ => true,
        }
    }
}

fn profile_allows_tool(profile: ToolProfile, tool_name: &str, planning_active: bool) -> bool {
    match profile {
        ToolProfile::CodexDefault => {
            matches!(
                tool_name,
                tools::EXEC_COMMAND | tools::WRITE_STDIN | tools::APPLY_PATCH
            ) || (planning_active
                && matches!(tool_name, tools::CODE_SEARCH | tools::REQUEST_USER_INPUT))
        }
        ToolProfile::AdvancedVtCode => !matches!(
            tool_name,
            tools::UNIFIED_EXEC
                | tools::UNIFIED_FILE
                | tools::UNIFIED_SEARCH
                | tools::LIST_FILES
                | tools::READ_FILE
                | tools::WRITE_FILE
                | tools::EDIT_FILE
                | tools::CREATE_FILE
                | tools::DELETE_FILE
                | tools::MOVE_FILE
                | tools::COPY_FILE
                | tools::SEARCH_REPLACE
                | tools::FILE_OP
        ),
    }
}

fn registration_catalog_source(
    registration: &ToolRegistration,
    kind: CatalogToolKind,
) -> ToolCatalogSource {
    if matches!(kind, CatalogToolKind::ApplyPatch) {
        return ToolCatalogSource::Builtin;
    }

    registration.catalog_source()
}

fn should_defer_tool_loading(entry: &ToolCatalogEntry, config: &SessionToolsConfig) -> bool {
    if !config.deferred_tool_policy.is_enabled() {
        return false;
    }

    if matches!(entry.source, ToolCatalogSource::Dynamic) {
        return false;
    }

    if config.deferred_tool_policy.keeps_entry_available(entry) || is_core_tool_entry(entry, config)
    {
        return false;
    }

    if config.deferred_tool_policy.is_client_local() {
        return matches!(entry.source, ToolCatalogSource::Mcp);
    }

    matches!(
        entry.source,
        ToolCatalogSource::Builtin | ToolCatalogSource::Mcp
    )
}

fn is_core_tool_entry(entry: &ToolCatalogEntry, config: &SessionToolsConfig) -> bool {
    // `entry.public_name` is always the canonical registration name, never an
    // alias (spawn_agent/spawn_background_subprocess/send_input/wait_agent/
    // resume_agent/close_agent all route to the single `agent` registration),
    // so only the canonical name needs to be matched here.
    match entry.public_name.as_str() {
        tools::EXEC_COMMAND
        | tools::WRITE_STDIN
        | tools::TASK_TRACKER
        | tools::START_PLANNING
        | tools::FINISH_PLANNING
        | tools::AGENT
        | tools::LIST_SKILLS
        | tools::LOAD_SKILL
        | tools::LOAD_SKILL_RESOURCE => true,
        tools::MCP_SEARCH_TOOLS | tools::MCP_GET_TOOL_DETAILS | tools::MCP_LIST_SERVERS => {
            config.deferred_tool_policy.is_client_local()
        }
        tools::MEMORY => config.anthropic_native_memory_enabled,
        tools::REQUEST_USER_INPUT => config.request_user_input_enabled,
        tools::APPLY_PATCH => config.model_capabilities.supports_apply_patch_tool,
        _ => false,
    }
}

fn surface_allows_tool(surface: SessionSurface, tool_name: &str) -> bool {
    match surface {
        SessionSurface::Interactive => !matches!(tool_name, tools::READ_FILE | tools::LIST_FILES),
        SessionSurface::AgentRunner => true,
        SessionSurface::Acp => matches!(
            tool_name,
            tools::EXEC_COMMAND | tools::WRITE_STDIN | tools::APPLY_PATCH | tools::CODE_SEARCH
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

fn default_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

/// Default max description length for MCP tool descriptions in Full mode.
/// MCP tools from external servers can have arbitrarily long descriptions;
/// capping them prevents token inflation.
const MCP_TOOL_DESCRIPTION_MAX_LEN: usize = 512;

fn compact_tool_description(
    original: &str,
    mode: ToolDocumentationMode,
    per_tool_max: Option<usize>,
) -> String {
    let mode_max = match mode {
        ToolDocumentationMode::Minimal => 64,
        ToolDocumentationMode::Progressive => 120,
        ToolDocumentationMode::Full => usize::MAX,
    };
    // Per-tool cap takes precedence over mode default
    let max_len = per_tool_max.unwrap_or(mode_max);

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

#[cfg(test)]
pub(crate) use vtcode_utility_tool_specs::{
    apply_patch_parameters, exec_command_parameters, list_files_parameters,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VTCodeConfig;
    use crate::tools::constants::empty_object_schema;
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
    fn default_profile_exposes_only_codex_baseline_tools() {
        let registrations = vec![
            registration(tools::EXEC_COMMAND)
                .with_description("Run command")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::WRITE_STDIN)
                .with_description("Write stdin")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::APPLY_PATCH)
                .with_llm_visibility(false)
                .with_description("Apply patch")
                .with_parameter_schema(apply_patch_parameters())
                .with_behavior(ToolBehavior::apply_patch(
                    ToolMutationModel::Mutating,
                    false,
                    true,
                )),
            registration(tools::CODE_SEARCH)
                .with_description("Search code")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::READ_FILE)
                .with_llm_visibility(false)
                .with_description("Read file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::WRITE_FILE)
                .with_llm_visibility(false)
                .with_description("Write file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::DELETE_FILE)
                .with_description("Delete file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::MOVE_FILE)
                .with_description("Move file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::COPY_FILE)
                .with_description("Copy file")
                .with_parameter_schema(empty_object_schema()),
            registration("ls")
                .with_description("List directory")
                .with_parameter_schema(empty_object_schema()),
            registration("rg")
                .with_description("Search text")
                .with_parameter_schema(empty_object_schema()),
            registration("find")
                .with_description("Find files")
                .with_parameter_schema(empty_object_schema()),
            registration("cat")
                .with_description("Print file")
                .with_parameter_schema(empty_object_schema()),
            registration("sed")
                .with_description("Stream edit")
                .with_parameter_schema(empty_object_schema()),
            registration("awk")
                .with_description("Process text")
                .with_parameter_schema(empty_object_schema()),
        ];

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let mut config = SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        );
        config.planning_active = false;
        let names = catalog.public_tool_names(config);

        assert_eq!(
            names,
            vec![
                tools::EXEC_COMMAND.to_string(),
                tools::WRITE_STDIN.to_string(),
                tools::APPLY_PATCH.to_string(),
            ]
        );
        for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
            assert!(
                !names.contains(&command.to_string()),
                "{command} must stay an exec_command.cmd example, not a default tool"
            );
        }
        for file_tool in [
            tools::READ_FILE,
            tools::WRITE_FILE,
            tools::DELETE_FILE,
            tools::MOVE_FILE,
            tools::COPY_FILE,
            tools::UNIFIED_FILE,
        ] {
            assert!(
                !names.contains(&file_tool.to_string()),
                "{file_tool} must stay out of the default file surface"
            );
        }
    }

    #[test]
    fn default_profile_exposes_planning_tools_during_planning() {
        let registrations = vec![
            registration(tools::CODE_SEARCH)
                .with_description("Search code")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::REQUEST_USER_INPUT)
                .with_description("Ask the user")
                .with_parameter_schema(empty_object_schema()),
        ];

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let names = catalog.public_tool_names(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_planning_active(true),
        );

        assert_eq!(
            names,
            vec![
                tools::CODE_SEARCH.to_string(),
                tools::REQUEST_USER_INPUT.to_string(),
            ]
        );
    }

    #[test]
    fn exec_command_schema_models_unix_tools_as_cmd_examples() {
        let registration = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(exec_command_parameters());
        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let entries = catalog.schema_entries(SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ));
        let entry = entries
            .iter()
            .find(|entry| entry.name == tools::EXEC_COMMAND)
            .expect("exec_command schema entry");
        let properties = &entry.parameters["properties"];

        assert_eq!(entry.parameters["required"], json!(["cmd"]));
        assert!(
            properties["cmd"]["description"]
                .as_str()
                .is_some_and(|text| {
                    ["ls", "rg", "find", "cat", "sed", "awk"]
                        .iter()
                        .all(|command| text.contains(command))
                })
        );
        assert_eq!(properties["tty"]["type"], "boolean");
        for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
            assert!(
                properties.get(command).is_none(),
                "{command} must not be modelled as a separate schema property"
            );
        }
    }

    #[test]
    fn advanced_profile_exposes_code_search_without_internal_search_names() {
        let registrations = vec![
            registration(tools::CODE_SEARCH)
                .with_description("Search code")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::LIST_FILES)
                .with_llm_visibility(false)
                .with_description("List files")
                .with_parameter_schema(list_files_parameters()),
            registration(tools::READ_FILE)
                .with_llm_visibility(false)
                .with_description("Read file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::WRITE_FILE)
                .with_llm_visibility(false)
                .with_description("Write file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::DELETE_FILE)
                .with_description("Delete file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::MOVE_FILE)
                .with_description("Move file")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::COPY_FILE)
                .with_description("Copy file")
                .with_parameter_schema(empty_object_schema()),
        ];

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let names = catalog.public_tool_names(
            SessionToolsConfig::full_public(
                SessionSurface::AgentRunner,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        );

        assert_eq!(names, vec![tools::CODE_SEARCH.to_string()]);
    }

    #[test]
    fn advanced_profile_retains_eligible_specialised_and_dynamic_tools() {
        let registrations = vec![
            registration(tools::CODE_SEARCH)
                .with_description("Search code")
                .with_parameter_schema(empty_object_schema()),
            registration("mcp::context7::search")
                .with_catalog_source(ToolCatalogSource::Mcp)
                .with_llm_visibility(false)
                .with_description("Search documentation")
                .with_parameter_schema(empty_object_schema())
                .with_aliases(["mcp__context7__search"]),
            registration(tools::LOAD_SKILL)
                .with_catalog_source(ToolCatalogSource::Builtin)
                .with_description("Load a skill")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::START_PLANNING)
                .with_catalog_source(ToolCatalogSource::Builtin)
                .with_description("Start planning")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::SPAWN_AGENT)
                .with_catalog_source(ToolCatalogSource::Builtin)
                .with_description("Spawn an agent")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::CRON_CREATE)
                .with_catalog_source(ToolCatalogSource::Builtin)
                .with_description("Create a scheduled prompt")
                .with_parameter_schema(empty_object_schema()),
            registration("dynamic_plugin_tool")
                .with_catalog_source(ToolCatalogSource::Dynamic)
                .with_description("Run a dynamic plugin tool")
                .with_parameter_schema(empty_object_schema()),
        ];

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let names = catalog.public_tool_names(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        );

        assert_eq!(
            names,
            vec![
                tools::CODE_SEARCH.to_string(),
                "mcp__context7__search".to_string(),
                tools::LOAD_SKILL.to_string(),
                tools::START_PLANNING.to_string(),
                tools::SPAWN_AGENT.to_string(),
                tools::CRON_CREATE.to_string(),
                "dynamic_plugin_tool".to_string(),
            ]
        );
    }

    #[test]
    fn acp_surface_exposes_code_search_with_advanced_profile() {
        let registrations = vec![
            registration(tools::EXEC_COMMAND)
                .with_description("Run command")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::WRITE_STDIN)
                .with_description("Write stdin")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::APPLY_PATCH)
                .with_llm_visibility(false)
                .with_description("Apply patch")
                .with_parameter_schema(apply_patch_parameters())
                .with_behavior(ToolBehavior::apply_patch(
                    ToolMutationModel::Mutating,
                    false,
                    true,
                )),
            registration(tools::CODE_SEARCH)
                .with_description("Search code")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::LOAD_SKILL)
                .with_description("Load a skill")
                .with_parameter_schema(empty_object_schema()),
        ];

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let names = catalog.public_tool_names(
            SessionToolsConfig::full_public(
                SessionSurface::Acp,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        );

        assert_eq!(
            names,
            vec![
                tools::EXEC_COMMAND.to_string(),
                tools::WRITE_STDIN.to_string(),
                tools::APPLY_PATCH.to_string(),
                tools::CODE_SEARCH.to_string(),
            ]
        );
    }

    #[test]
    fn rebuild_catalog_uses_public_mcp_alias() {
        let registration = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let names = catalog.public_tool_names(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        );

        assert_eq!(names, vec!["mcp__context7__search".to_string()]);
    }

    #[test]
    fn schema_entries_hide_request_user_input_when_disabled() {
        let registration = registration(tools::REQUEST_USER_INPUT)
            .with_description("Ask the user")
            .with_parameter_schema(empty_object_schema());

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let names = catalog.public_tool_names(SessionToolsConfig {
            surface: SessionSurface::Interactive,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: ToolDocumentationMode::Full,
            planning_active: true,
            request_user_input_enabled: false,
            model_capabilities: ToolModelCapabilities::default(),
            deferred_tool_policy: DeferredToolPolicy::default(),
            anthropic_native_memory_enabled: false,
            tool_profile: ToolProfile::CodexDefault,
        });

        assert!(names.is_empty());
    }

    #[test]
    fn task_tracker_stays_visible_outside_planning_workflow() {
        let registration = registration(tools::TASK_TRACKER)
            .with_description("Track plan tasks")
            .with_parameter_schema(empty_object_schema());

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
        let names = catalog.public_tool_names(SessionToolsConfig {
            surface: SessionSurface::Interactive,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: ToolDocumentationMode::Full,
            planning_active: false,
            request_user_input_enabled: true,
            model_capabilities: ToolModelCapabilities::default(),
            deferred_tool_policy: DeferredToolPolicy::default(),
            anthropic_native_memory_enabled: false,
            tool_profile: ToolProfile::AdvancedVtCode,
        });

        assert_eq!(names, vec![tools::TASK_TRACKER.to_string()]);
    }

    #[test]
    fn memory_tool_is_hidden_unless_anthropic_native_memory_is_enabled() {
        let registration = registration(tools::MEMORY)
            .with_description("Native memory")
            .with_parameter_schema(empty_object_schema());
        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);

        let hidden = catalog.public_tool_names(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ));
        assert!(hidden.is_empty());

        let visible = catalog.public_tool_names(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_anthropic_native_memory_enabled(true),
        );
        assert_eq!(visible, vec![tools::MEMORY.to_string()]);
    }

    #[test]
    fn memory_tool_uses_anthropic_native_definition_when_visible() {
        let registration = registration(tools::MEMORY)
            .with_description("Native memory")
            .with_parameter_schema(empty_object_schema());
        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);

        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_anthropic_native_memory_enabled(true),
        );

        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].tool_type, "memory_20250818");
        assert_eq!(definitions[0].function_name(), tools::MEMORY);
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
    fn agent_runner_default_hides_legacy_browse_tools() {
        let read_file = registration(tools::READ_FILE)
            .with_llm_visibility(false)
            .with_description("Read file contents in chunks")
            .with_parameter_schema(empty_object_schema());
        let list_files = registration(tools::LIST_FILES)
            .with_llm_visibility(false)
            .with_description("List files with pagination")
            .with_parameter_schema(list_files_parameters());
        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![read_file, list_files]);

        let interactive_names = catalog.public_tool_names(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ));
        assert!(!interactive_names.contains(&tools::READ_FILE.to_string()));
        assert!(!interactive_names.contains(&tools::LIST_FILES.to_string()));

        let agent_runner_names = catalog.public_tool_names(SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ));
        assert!(!agent_runner_names.contains(&tools::READ_FILE.to_string()));
        assert!(!agent_runner_names.contains(&tools::LIST_FILES.to_string()));
    }

    #[test]
    fn parallel_support_comes_from_behavior_metadata() {
        let registration = registration("parallel_catalog_tool")
            .with_description("parallel-safe test tool")
            .with_parameter_schema(empty_object_schema())
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

    #[test]
    fn anthropic_policy_injects_tool_search_and_defers_non_core_tools() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());
        let apply_patch = registration(tools::APPLY_PATCH)
            .with_llm_visibility(false)
            .with_description("Apply patch")
            .with_parameter_schema(apply_patch_parameters())
            .with_behavior(ToolBehavior::apply_patch(
                ToolMutationModel::Mutating,
                false,
                true,
            ));
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let mut registrations = vec![exec_command, apply_patch, mcp_tool];
        for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
            let name: &'static str =
                Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
            let alias = format!("mcp__context7__resolve_{index}");
            registrations.push(
                registration(name)
                    .with_catalog_source(ToolCatalogSource::Mcp)
                    .with_llm_visibility(false)
                    .with_description(format!("resolve docs {index}"))
                    .with_parameter_schema(empty_object_schema())
                    .with_aliases([alias]),
            );
        }

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::anthropic(
                ToolSearchAlgorithm::Regex,
                Vec::new(),
            )),
        );

        assert!(
            definitions
                .iter()
                .any(|tool| tool.tool_type == "tool_search_tool_regex_20251119"),
            "anthropic tool search should be injected when deferred tools exist"
        );
        let exec_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == tools::EXEC_COMMAND)
            .expect("exec_command should be present");
        assert_eq!(exec_tool.defer_loading, None);

        let apply_patch = definitions
            .iter()
            .find(|tool| tool.function_name() == tools::APPLY_PATCH)
            .expect("apply_patch fallback should be present");
        assert_eq!(apply_patch.defer_loading, Some(true));

        let mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("mcp tool should be present");
        assert_eq!(mcp_tool.defer_loading, Some(true));
    }

    #[test]
    fn mcp_tool_registration_derives_namespace_from_server_name() {
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![mcp_tool]);
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == "mcp__context7__search")
            .expect("mcp entry should be present");

        let namespace = entry
            .namespace
            .as_ref()
            .expect("mcp tool should derive a namespace from its server name");
        assert_eq!(namespace.name, "context7");
        assert_eq!(
            namespace.description,
            "Tools provided by MCP server 'context7'"
        );
    }

    #[test]
    fn core_tool_registration_has_no_namespace() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command]);
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == tools::EXEC_COMMAND)
            .expect("core tool entry should be present");

        assert!(
            entry.namespace.is_none(),
            "core/builtin tools should not derive a namespace"
        );
    }

    #[test]
    fn model_tools_attach_namespace_only_to_deferred_mcp_tools() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let mut registrations = vec![exec_command, mcp_tool];
        for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
            let name: &'static str =
                Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
            let alias = format!("mcp__context7__resolve_{index}");
            registrations.push(
                registration(name)
                    .with_catalog_source(ToolCatalogSource::Mcp)
                    .with_llm_visibility(false)
                    .with_description(format!("resolve docs {index}"))
                    .with_parameter_schema(empty_object_schema())
                    .with_aliases([alias]),
            );
        }

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::anthropic(
                ToolSearchAlgorithm::Regex,
                Vec::new(),
            )),
        );

        let core_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == tools::EXEC_COMMAND)
            .expect("exec_command should be present");
        assert_eq!(core_tool.defer_loading, None);
        assert!(
            core_tool.namespace.is_none(),
            "non-deferred core tools should never carry namespace metadata"
        );

        let deferred_mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("deferred mcp tool should be present");
        assert_eq!(deferred_mcp_tool.defer_loading, Some(true));
        let namespace = deferred_mcp_tool
            .namespace
            .as_ref()
            .expect("deferred mcp tool should carry namespace metadata");
        assert_eq!(namespace.name, "context7");
    }

    #[test]
    fn small_mcp_catalog_is_deferred_despite_low_tool_count() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_tool]);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::anthropic(
                ToolSearchAlgorithm::Regex,
                Vec::new(),
            )),
        );

        let mcp_definition = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("mcp tool should be present");
        assert_eq!(
            mcp_definition.defer_loading,
            Some(true),
            "even a single MCP tool should be deferred to avoid schema tax"
        );
    }

    #[test]
    fn client_local_policy_deferred_for_small_mcp_catalog() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());
        let mcp_search_tools = registration(tools::MCP_SEARCH_TOOLS)
            .with_description("Search MCP tools")
            .with_parameter_schema(empty_object_schema());
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![
            exec_command,
            mcp_search_tools,
            mcp_tool,
        ]);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new())),
        );

        assert!(
            definitions
                .iter()
                .any(|tool| tool.function_name() == "mcp__context7__search"),
            "mcp tool should still be listed in the model-facing catalog for client-local search"
        );
        let mcp_definition = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("mcp tool should be present");
        assert_eq!(
            mcp_definition.defer_loading,
            Some(true),
            "client-local deferral should also apply to small MCP catalogs"
        );
        let search_definition = definitions
            .iter()
            .find(|tool| tool.function_name() == tools::MCP_SEARCH_TOOLS)
            .expect("client-local MCP search should remain available");
        assert_eq!(search_definition.defer_loading, None);
    }

    #[test]
    fn openai_policy_injects_tool_search_for_large_catalogs() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let mut registrations = vec![exec_command, mcp_tool];
        for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
            let name: &'static str =
                Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
            let alias = format!("mcp__context7__resolve_{index}");
            registrations.push(
                registration(name)
                    .with_catalog_source(ToolCatalogSource::Mcp)
                    .with_llm_visibility(false)
                    .with_description(format!("resolve docs {index}"))
                    .with_parameter_schema(empty_object_schema())
                    .with_aliases([alias]),
            );
        }

        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities {
                    supports_apply_patch_tool: true,
                },
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(vec![
                "mcp__context7__search".to_string(),
            ])),
        );

        assert!(
            definitions
                .iter()
                .any(|tool| tool.tool_type == "tool_search"),
            "openai hosted tool search should be injected when deferred tools exist"
        );
        let mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("mcp tool should be present");
        assert_eq!(mcp_tool.defer_loading, None);

        let deferred_mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__resolve_0")
            .expect("deferred mcp tool should be present");
        assert_eq!(deferred_mcp_tool.defer_loading, Some(true));
    }

    #[test]
    fn openai_policy_deferred_for_small_mcp_catalog() {
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);
        let second_mcp_tool = registration("mcp::context7::resolve")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("resolve docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__resolve"]);

        let catalog =
            SessionToolCatalog::rebuild_from_registrations(vec![mcp_tool, second_mcp_tool]);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(vec![
                "mcp__context7__search".to_string(),
            ])),
        );

        assert!(
            definitions
                .iter()
                .any(|tool| tool.tool_type == "tool_search"),
            "MCP presence should trigger tool search even for a small catalog"
        );
        let mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("mcp tool should be present");
        assert_eq!(
            mcp_tool.defer_loading, None,
            "always-available tool stays eager"
        );

        let direct_mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__resolve")
            .expect("deferred mcp tool should be present");
        assert_eq!(
            direct_mcp_tool.defer_loading,
            Some(true),
            "non-always-available MCP tool should be deferred"
        );
    }

    #[test]
    fn always_available_tools_match_registration_names_and_aliases() {
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);
        let dynamic_tool = registration("dynamic_skill_tool")
            .with_description("dynamic skill tool")
            .with_parameter_schema(empty_object_schema());

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![mcp_tool, dynamic_tool]);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode)
            .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(vec![
                "mcp::context7::search".to_string(),
                "dynamic_skill_tool".to_string(),
            ])),
        );

        let mcp_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "mcp__context7__search")
            .expect("mcp tool should be present");
        assert_eq!(mcp_tool.defer_loading, None);

        let dynamic_tool = definitions
            .iter()
            .find(|tool| tool.function_name() == "dynamic_skill_tool")
            .expect("dynamic tool should be present");
        assert_eq!(dynamic_tool.defer_loading, None);
    }

    #[test]
    fn unsupported_providers_keep_catalog_eager() {
        let exec_command = registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema());
        let mcp_tool = registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]);

        let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_tool]);
        let definitions = catalog.model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities::default(),
            )
            .with_tool_profile(ToolProfile::AdvancedVtCode),
        );

        assert!(!definitions.iter().any(|tool| tool.is_tool_search()));
        assert!(
            definitions.iter().all(|tool| tool.defer_loading.is_none()),
            "unsupported providers should keep the eager catalog"
        );
    }

    #[test]
    fn deferred_tool_policy_uses_provider_defaults() {
        let config = VTCodeConfig::default();

        let anthropic =
            deferred_tool_policy_for_runtime(Some(Provider::Anthropic), false, Some(&config));
        assert!(anthropic.is_enabled());
        assert_eq!(
            anthropic
                .tool_search_definition()
                .map(|tool| tool.tool_type),
            Some("tool_search_tool_regex_20251119".to_string())
        );

        let openai = deferred_tool_policy_for_runtime(Some(Provider::OpenAI), true, Some(&config));
        assert!(openai.is_enabled());
        assert_eq!(
            openai.tool_search_definition().map(|tool| tool.tool_type),
            Some("tool_search".to_string())
        );

        // OpenAI without Responses compaction, and no explicit provider-hosted
        // tool search, falls through to client-local deferral now that
        // `client_tool_search` defaults to `true`.
        let unsupported =
            deferred_tool_policy_for_runtime(Some(Provider::OpenAI), false, Some(&config));
        assert!(unsupported.is_enabled());
        assert!(unsupported.is_client_local());
    }

    #[test]
    fn client_local_policy_selected_when_flag_enabled_for_unsupported_provider() {
        let mut config = VTCodeConfig::default();
        config.tools.client_tool_search = true;

        let gemini = deferred_tool_policy_for_runtime(Some(Provider::Gemini), false, Some(&config));
        assert!(gemini.is_enabled());
        assert!(gemini.is_client_local());
        assert_eq!(gemini.tool_search_definition(), None);

        // No provider inferred (e.g. unknown/custom model) is also covered
        // by the fallthrough arm.
        let no_provider = deferred_tool_policy_for_runtime(None, false, Some(&config));
        assert!(no_provider.is_enabled());
        assert!(no_provider.is_client_local());
    }

    #[test]
    fn client_local_policy_not_selected_when_flag_disabled() {
        let mut config = VTCodeConfig::default();
        // Default is enabled; explicitly disable it to test the fallback path.
        config.tools.client_tool_search = false;
        assert!(!config.tools.client_tool_search);

        let gemini = deferred_tool_policy_for_runtime(Some(Provider::Gemini), false, Some(&config));
        assert!(!gemini.is_enabled());
        assert!(!gemini.is_client_local());

        let no_config = deferred_tool_policy_for_runtime(Some(Provider::Gemini), false, None);
        assert!(!no_config.is_enabled());
        assert!(!no_config.is_client_local());
    }

    #[test]
    fn anthropic_native_memory_runtime_flag_tracks_provider_and_config() {
        let mut config = VTCodeConfig::default();
        config.provider.anthropic.memory.enabled = true;

        assert!(anthropic_native_memory_enabled_for_runtime(
            Some(Provider::Anthropic),
            "claude-sonnet-4-6",
            Some(&config),
        ));
        assert!(!anthropic_native_memory_enabled_for_runtime(
            Some(Provider::OpenAI),
            "claude-sonnet-4-6",
            Some(&config),
        ));
        assert!(!anthropic_native_memory_enabled_for_runtime(
            Some(Provider::Anthropic),
            "gpt-5",
            Some(&config),
        ));
        assert!(anthropic_native_memory_enabled_for_runtime(
            Some(Provider::Anthropic),
            "my-private-claude-build",
            Some(&config),
        ));
    }
}
