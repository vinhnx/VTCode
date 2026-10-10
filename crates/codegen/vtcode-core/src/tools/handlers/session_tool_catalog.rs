use crate::config::ModelId;
use crate::config::ToolDocumentationMode;
use crate::config::constants::tools;
use crate::config::loader::VTCodeConfig;
use crate::config::models::Provider;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::{ToolDefinition, ToolNamespace, ToolSearchAlgorithm};
use crate::llm::providers::gemini::wire::FunctionDeclaration;
use crate::tool_policy::ToolPolicy;
#[cfg(test)]
use crate::tools::handlers::compact::compact_tool_description;
use crate::tools::handlers::compact::{MCP_TOOL_DESCRIPTION_MAX_LEN, compact_parameters};
use crate::tools::mcp::MCP_QUALIFIED_TOOL_PREFIX;
use crate::tools::registry::{ToolHandler as RegistryToolHandler, ToolRegistration};
use crate::tools::tool_intent::ToolSurfaceKind;
use crate::utils::tool_name_parsing::parse_canonical_mcp_tool_name;
use rustc_hash::FxHashSet;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};
use vtcode_utility_tool_specs::{parse_tool_input_schema, with_max_output_tokens_parameter};

use super::session_tool_projection::{ToolEntryProjection, ToolProjectionCache};
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

/// Whether a catalog is large enough that deferred loading would engage for it
/// (MCP tools present, at/over the count threshold, or over the schema-token
/// budget). Mirrors the "would defer" branch of the `model_tools` decision -- it
/// is the negation of `(count < THRESHOLD && !has_mcp && tokens <= BUDGET)`.
///
/// Used to decide whether to warn when deferred loading is *disabled*
/// (`tools.client_tool_search = false` on a provider without hosted tool
/// search): in that case the user pays the full schema tax a catalog this size
/// would otherwise shed. Extracted as pure logic so the warning condition is
/// unit-testable without capturing tracing output.
#[must_use]
fn catalog_would_benefit_from_deferral(
    has_mcp_tools: bool,
    deferable_tool_count: usize,
    estimated_schema_tokens: usize,
) -> bool {
    has_mcp_tools
        || deferable_tool_count >= DIRECT_TOOL_EXPOSURE_THRESHOLD
        || estimated_schema_tokens > DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET
}

/// Policy for deferred tool loading (tool search).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeferredToolPolicy {
    search_kind: Option<DeferredToolSearchKind>,
    always_available_tools: BTreeSet<String>,
}

impl DeferredToolPolicy {
    /// Creates a policy for Anthropic's tool search.
    #[must_use]
    pub fn anthropic(algorithm: ToolSearchAlgorithm, always_available_tools: impl IntoIterator<Item = String>) -> Self {
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
        self.always_available_tools.contains(entry.public_name.as_str())
            || self.always_available_tools.contains(entry.registration_name.as_str())
            || entry
                .aliases
                .iter()
                .any(|alias| self.always_available_tools.contains(alias.as_str()))
    }

    fn tool_search_definition(&self) -> Option<ToolDefinition> {
        match self.search_kind {
            Some(DeferredToolSearchKind::Anthropic(algorithm)) => Some(ToolDefinition::tool_search(algorithm)),
            Some(DeferredToolSearchKind::OpenAIHosted) => Some(ToolDefinition::hosted_tool_search()),
            // Client-local discovery is a core capability; there is
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
            let enabled = vtcode_config.is_none_or(|cfg| cfg.provider.anthropic.tool_search.enabled);
            let defer_by_default = vtcode_config.is_none_or(|cfg| cfg.provider.anthropic.tool_search.defer_by_default);
            if !enabled || !defer_by_default {
                return DeferredToolPolicy::default();
            }

            let algorithm = vtcode_config
                .map(|cfg| cfg.provider.anthropic.tool_search.algorithm)
                .unwrap_or_default();
            let always_available_tools = vtcode_config
                .map(|cfg| cfg.provider.anthropic.tool_search.always_available_tools.clone())
                .unwrap_or_default();
            DeferredToolPolicy::anthropic(algorithm, always_available_tools)
        }
        Some(Provider::OpenAI) if model_supports_responses_compaction => {
            let enabled = vtcode_config.is_none_or(|cfg| cfg.provider.openai.tool_search.enabled);
            let defer_by_default = vtcode_config.is_none_or(|cfg| cfg.provider.openai.tool_search.defer_by_default);
            if !enabled || !defer_by_default {
                return DeferredToolPolicy::default();
            }

            let always_available_tools = vtcode_config
                .map(|cfg| cfg.provider.openai.tool_search.always_available_tools.clone())
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
            let client_tool_search_enabled = vtcode_config.is_some_and(|cfg| cfg.tools.client_tool_search);
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
            tool_profile: ToolProfile::VtCode,
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
#[derive(Debug, Clone)]
pub struct SessionToolCatalog {
    entries: Vec<ToolCatalogEntry>,
    projection_cache: Arc<ToolProjectionCache>,
}

impl Default for SessionToolCatalog {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl SessionToolCatalog {
    /// Creates a new catalog from the given entries.
    pub fn new(entries: Vec<ToolCatalogEntry>) -> Self {
        let projection_cache = Arc::new(ToolProjectionCache::new(entries.len()));
        Self { entries, projection_cache }
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
        Self::new(entries)
    }

    /// Returns the names of all public tools visible with the given config.
    pub fn public_tool_names(&self, config: SessionToolsConfig) -> Vec<String> {
        self.visible_entry_indices(&config)
            .into_iter()
            .map(|index| self.entries[index].public_name.clone())
            .collect()
    }

    /// Returns schema entries for all visible tools.
    pub fn schema_entries(&self, config: SessionToolsConfig) -> Vec<ToolSchemaEntry> {
        self.visible_entry_indices(&config)
            .into_iter()
            .map(|index| {
                let entry = &self.entries[index];
                let projection = self.projection(index, entry, config.documentation_mode);
                ToolSchemaEntry {
                    name: entry.public_name.clone(),
                    description: self.description_for_entry(entry, projection, &config).to_owned(),
                    parameters: self.parameters_for_entry(entry, projection, &config),
                }
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
        let visible_entry_indices = self.visible_entry_indices(&config);
        let deferable_tool_count = visible_entry_indices
            .iter()
            .filter(|&&index| should_defer_tool_loading(&self.entries[index], &config))
            .count();
        let has_mcp_tools = visible_entry_indices
            .iter()
            .any(|&index| matches!(self.entries[index].source, ToolCatalogSource::Mcp));
        // Hosted policies (Anthropic/OpenAI) rely on `defer_loading` markers in
        // the wire payload, so they defer whenever the policy is enabled. The
        // client-local policy instead OMITS deferred definitions from the wire;
        // applying it to a small builtin-only catalog (e.g. the plan-mode
        // read-only filter) can leave zero tools on the wire, which makes
        // models improvise textual XML tool calls. Below the exposure
        // threshold, eager exposure is cheaper and simpler -- exactly the gate
        // documented on `DIRECT_TOOL_EXPOSURE_THRESHOLD` and the
        // `deferred_tool_policy_for_runtime` fallback arm.
        let expose_tools_directly = if !config.deferred_tool_policy.is_enabled() {
            // The disabled policy needs the estimate for its advisory warning.
            let estimated_schema_tokens = self.estimate_schema_tokens(&visible_entry_indices, &config);

            // Advisory one-shot: if deferred loading is disabled but this catalog
            // is large enough that deferral would engage, the user is paying the
            // full tool-schema tax on every request they could shed by enabling
            // `tools.client_tool_search`. Warn once per process -- the config
            // choice is stable across a session and repeating per request is noise.
            // (The system-prompt-exceeds-budget warning is emitted separately in
            // `prompts::system::apply_token_budget`; this covers the tool side.)
            if catalog_would_benefit_from_deferral(has_mcp_tools, deferable_tool_count, estimated_schema_tokens) {
                static DEFERRAL_DISABLED_WARNING: OnceLock<()> = OnceLock::new();
                if DEFERRAL_DISABLED_WARNING.set(()).is_ok() {
                    tracing::warn!(
                        available_tools = visible_entry_indices.len(),
                        deferable_tools = deferable_tool_count,
                        has_mcp_tools,
                        estimated_schema_tokens,
                        threshold = DIRECT_TOOL_EXPOSURE_THRESHOLD,
                        token_budget = DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET,
                        "Deferred tool loading is disabled (tools.client_tool_search = false) \
                         but the catalog is large enough to benefit from it; the full tool \
                         schema tax is paid on every request. Enable tools.client_tool_search \
                         to omit MCP/large schemas from the wire payload until needed."
                    );
                }
            }

            true
        } else if config.deferred_tool_policy.is_client_local() {
            // Client-local deferral uses the estimate to decide whether omitting
            // the deferred definitions is worthwhile for this catalog.
            let estimated_schema_tokens = self.estimate_schema_tokens(&visible_entry_indices, &config);
            !catalog_would_benefit_from_deferral(has_mcp_tools, deferable_tool_count, estimated_schema_tokens)
        } else {
            // Hosted policies always keep their deferred definitions in the wire
            // payload and therefore never need the schema-token estimate.
            false
        };

        let mut tools = Vec::with_capacity(visible_entry_indices.len() + if expose_tools_directly { 0 } else { 1 });
        let mut has_deferred_tools = false;

        for index in visible_entry_indices {
            let entry = &self.entries[index];
            let projection = self.projection(index, entry, config.documentation_mode);
            let defer_loading = should_defer_tool_loading(entry, &config);
            match entry.kind {
                CatalogToolKind::ApplyPatch if config.model_capabilities.supports_apply_patch_tool => {
                    let mut tool =
                        ToolDefinition::apply_patch(self.description_for_entry(entry, projection, &config).to_owned());
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
                            self.description_for_entry(entry, projection, &config).to_owned(),
                            self.parameters_for_entry(entry, projection, &config),
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

        if has_deferred_tools && let Some(search_tool) = config.deferred_tool_policy.tool_search_definition() {
            tools.push(search_tool);
        }

        tools
    }

    /// Returns the schema entry for a tool by name.
    pub fn schema_for_name(&self, name: &str, config: SessionToolsConfig) -> Option<ToolSchemaEntry> {
        self.schema_entries(config).into_iter().find(|entry| entry.name == name)
    }

    pub(crate) fn entries(&self) -> &[ToolCatalogEntry] {
        &self.entries
    }

    fn estimate_schema_tokens(&self, entry_indices: &[usize], config: &SessionToolsConfig) -> usize {
        entry_indices
            .iter()
            .map(|&index| {
                let entry = &self.entries[index];
                let projection = self.projection(index, entry, config.documentation_mode);
                if entry.public_name == tools::TASK_TRACKER {
                    serialized_schema_token_estimate(
                        entry.public_name.as_str(),
                        self.description_for_entry(entry, projection, config),
                        &self.parameters_for_entry(entry, projection, config),
                    )
                } else {
                    projection.serialized_token_estimate(entry.public_name.as_str())
                }
            })
            .sum()
    }

    fn projection(
        &self,
        entry_index: usize,
        entry: &ToolCatalogEntry,
        documentation_mode: ToolDocumentationMode,
    ) -> &ToolEntryProjection {
        self.projection_cache.get_or_init(entry_index, entry, documentation_mode)
    }

    fn parameters_for_entry(
        &self,
        entry: &ToolCatalogEntry,
        projection: &ToolEntryProjection,
        config: &SessionToolsConfig,
    ) -> Value {
        if entry.public_name == tools::TASK_TRACKER {
            return compact_parameters(
                with_max_output_tokens_parameter(super::task_tracker::task_tracker_parameter_schema_for_workflow(
                    config.planning_active,
                )),
                config.documentation_mode,
            );
        }

        projection.parameters().clone()
    }

    fn description_for_entry<'a>(
        &self,
        entry: &'a ToolCatalogEntry,
        projection: &'a ToolEntryProjection,
        config: &SessionToolsConfig,
    ) -> &'a str {
        if entry.public_name == tools::TASK_TRACKER {
            return super::task_tracker::task_tracker_description_for_workflow(config.planning_active);
        }

        projection.description()
    }

    fn visible_entry_indices(&self, config: &SessionToolsConfig) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.is_visible(config).then_some(index))
            .collect()
    }
}

fn serialized_schema_token_estimate(name: &str, description: &str, parameters: &Value) -> usize {
    serde_json::to_string(&json!({
        "name": name,
        "description": description,
        "parameters": parameters,
    }))
    .map(|serialized| serialized.len() / 4)
    .unwrap_or(0)
}

impl ToolCatalogEntry {
    fn from_registration(registration: &ToolRegistration) -> Option<Self> {
        let metadata = registration.metadata();
        let description = metadata.description()?.to_string();
        let parameters = with_max_output_tokens_parameter(metadata.parameter_schema().cloned().unwrap_or_else(|| {
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
            })
        }));
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

    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
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

        if !profile_allows_tool(config.tool_profile, self.public_name.as_str(), config.planning_active) {
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
    if tool_name == tools::MATRIX {
        return true;
    }
    match profile {
        // Planning keeps every read-only inspection surface plus the
        // interview tool. A `code_search`-only catalog (turn_912/913) forced
        // planners to answer basic "read this file / list this directory"
        // questions with repeated fuzzy searches. Mutating tools stay
        // excluded; the planning dispatch gate also hard-blocks mutations.
        ToolProfile::VtCode if planning_active => matches!(
            tool_name,
            tools::EXEC_COMMAND
                | tools::READ_FILE
                | tools::LIST_FILES
                | tools::GREP_FILE
                | tools::CODE_SEARCH
                | tools::REQUEST_USER_INPUT
                | tools::RECORD_DECISION
        ),
        ToolProfile::VtCode => {
            matches!(
                tool_name,
                tools::EXEC_COMMAND
                    | tools::WRITE_STDIN
                    | tools::APPLY_PATCH
                    | tools::SEARCH_TOOLS
                    | tools::RECORD_DECISION
            )
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

fn registration_catalog_source(registration: &ToolRegistration, kind: CatalogToolKind) -> ToolCatalogSource {
    if matches!(kind, CatalogToolKind::ApplyPatch) {
        return ToolCatalogSource::Builtin;
    }

    registration.catalog_source()
}

fn should_defer_tool_loading(entry: &ToolCatalogEntry, config: &SessionToolsConfig) -> bool {
    if !config.deferred_tool_policy.is_enabled() {
        return false;
    }

    if config.deferred_tool_policy.keeps_entry_available(entry) || is_core_tool_entry(entry, config) {
        return false;
    }

    if config.deferred_tool_policy.is_client_local() {
        return matches!(
            entry.source,
            ToolCatalogSource::Builtin | ToolCatalogSource::Mcp | ToolCatalogSource::Dynamic
        );
    }

    matches!(entry.source, ToolCatalogSource::Builtin | ToolCatalogSource::Mcp | ToolCatalogSource::Dynamic)
}

fn is_core_tool_entry(entry: &ToolCatalogEntry, config: &SessionToolsConfig) -> bool {
    // `entry.public_name` is always the canonical registration name, never an
    // alias (spawn_agent/spawn_background_subprocess/send_input/wait_agent/
    // resume_agent/close_agent all route to the single `agent` registration),
    // so only the canonical name needs to be matched here.
    //
    // Always-eager set is the Codex baseline plus structured search
    // (HarnessTax lean defaults, revised 2026-09-28): `exec_command`,
    // `write_stdin`, `search_tools`, `apply_patch` in its supported representation, and
    // `code_search`/`grep_file`. Session data showed deferred structured
    // search is worse for cost than the extra schema tokens — models shell
    // out via `rg`/`git` and feed huge outputs into the prompt.
    match entry.public_name.as_str() {
        tools::EXEC_COMMAND
        | tools::WRITE_STDIN
        | tools::SEARCH_TOOLS
        | tools::CODE_SEARCH
        | tools::GREP_FILE
        | tools::MATRIX => true,
        // Planning keeps its workflow surface on the wire even when MCP tools
        // force deferral. Structured search (code_search/grep_file) is
        // always-eager above; read_file/list_files stay planning-only.
        tools::TASK_TRACKER | tools::START_PLANNING if config.planning_active => true,
        tools::READ_FILE | tools::LIST_FILES if config.planning_active => true,
        tools::MCP_SEARCH_TOOLS | tools::MCP_GET_TOOL_DETAILS | tools::MCP_LIST_SERVERS => {
            config.deferred_tool_policy.is_client_local()
        }
        tools::MEMORY => config.anthropic_native_memory_enabled,
        tools::REQUEST_USER_INPUT => config.request_user_input_enabled,
        tools::APPLY_PATCH => true,
        _ => false,
    }
}

fn surface_allows_tool(surface: SessionSurface, tool_name: &str) -> bool {
    match surface {
        SessionSurface::Interactive => !matches!(tool_name, tools::READ_FILE | tools::LIST_FILES),
        SessionSurface::AgentRunner => true,
        SessionSurface::Acp => {
            matches!(tool_name, tools::EXEC_COMMAND | tools::WRITE_STDIN | tools::APPLY_PATCH | tools::CODE_SEARCH)
        }
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

#[cfg(test)]
use vtcode_utility_tool_specs::{apply_patch_parameters, exec_command_parameters, list_files_parameters};

#[cfg(test)]
mod tests;
