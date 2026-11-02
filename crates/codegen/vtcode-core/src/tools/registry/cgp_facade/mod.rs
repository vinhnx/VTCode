//! CGP integration facade for ToolRegistry.
//!
//! Provides `enable_cgp_pipeline()` which prefers tool-specific native CGP
//! facades when available and otherwise wraps registered `TraitObject` tools
//! through the CGP approval → sandbox → logging/cache/retry pipeline while
//! preserving registration-sourced metadata.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use super::ToolRegistry;
use super::registration::{ToolExecutorFn, ToolHandler, ToolRegistration};
use crate::components::{wrap_native_tool_ci, wrap_native_tool_interactive, wrap_tool_ci, wrap_tool_interactive};
use crate::tool_policy::ToolPolicy;
use crate::tools::result::ToolResult as SplitToolResult;
use crate::tools::traits::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

fn registration_uses_live_task_tracker_metadata(registration: &ToolRegistration) -> bool {
    registration.name() == crate::config::constants::tools::TASK_TRACKER
}

fn leak_pattern_str(value: impl Into<String>) -> &'static str {
    Box::leak(value.into().into_boxed_str())
}

fn leak_patterns(patterns: &[String]) -> Option<&'static [&'static str]> {
    if patterns.is_empty() {
        return None;
    }

    let leaked_patterns = patterns
        .iter()
        .cloned()
        .map(leak_pattern_str)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Some(Box::leak(leaked_patterns))
}

#[derive(Clone)]
struct RegistrationMetadataSnapshot {
    name: Arc<str>,
    description: Arc<str>,
    parameter_schema: Option<Value>,
    config_schema: Option<Value>,
    state_schema: Option<Value>,
    prompt_path: Option<String>,
    default_permission: ToolPolicy,
    allow_patterns: Option<&'static [&'static str]>,
    deny_patterns: Option<&'static [&'static str]>,
}

impl RegistrationMetadataSnapshot {
    fn from_registration(registration: &ToolRegistration) -> Self {
        Self {
            name: Arc::<str>::from(registration.name()),
            description: Arc::<str>::from(registration.metadata().description().unwrap_or_default()),
            parameter_schema: registration.parameter_schema().cloned(),
            config_schema: registration.config_schema().cloned(),
            state_schema: registration.state_schema().cloned(),
            prompt_path: registration.prompt_path().map(str::to_string),
            default_permission: registration.default_permission().unwrap_or(ToolPolicy::Prompt),
            allow_patterns: leak_patterns(registration.metadata().allowlist()),
            deny_patterns: leak_patterns(registration.metadata().denylist()),
        }
    }

    fn from_registration_with_tool<T>(registration: &ToolRegistration, tool: &T) -> Self
    where
        T: Tool + ?Sized,
    {
        let use_live_task_tracker_metadata = registration_uses_live_task_tracker_metadata(registration);
        Self {
            name: Arc::<str>::from(registration.name()),
            description: if use_live_task_tracker_metadata {
                Arc::<str>::from(tool.description())
            } else {
                registration
                    .metadata()
                    .description()
                    .map(Arc::<str>::from)
                    .unwrap_or_else(|| Arc::<str>::from(tool.description()))
            },
            parameter_schema: if use_live_task_tracker_metadata {
                tool.parameter_schema()
            } else {
                registration.parameter_schema().cloned().or_else(|| tool.parameter_schema())
            },
            config_schema: registration.config_schema().cloned().or_else(|| tool.config_schema()),
            state_schema: registration.state_schema().cloned().or_else(|| tool.state_schema()),
            prompt_path: registration
                .prompt_path()
                .map(str::to_string)
                .or_else(|| tool.prompt_path().map(Cow::into_owned)),
            default_permission: registration.default_permission().unwrap_or_else(|| tool.default_permission()),
            allow_patterns: leak_patterns(registration.metadata().allowlist()).or_else(|| tool.allow_patterns()),
            deny_patterns: leak_patterns(registration.metadata().denylist()).or_else(|| tool.deny_patterns()),
        }
    }

    /// Shared metadata accessors for the three wrapper tools below.
    ///
    /// These bodies are intentionally identical across `RegistryFnTool`,
    /// `RegistrationBackedTool`, and `RegistrationBackedDynTool`: keep them
    /// here so a future per-wrapper edit cannot silently diverge one copy.
    /// `description()` / `parameter_schema()` stay per-impl: they carry the
    /// task_tracker live-metadata branch, which `RegistryFnTool` cannot
    /// share (it holds no inner tool to delegate to).
    fn name(&self) -> &str {
        self.name.as_ref()
    }

    fn config_schema(&self) -> Option<Value> {
        self.config_schema.clone()
    }

    fn state_schema(&self) -> Option<Value> {
        self.state_schema.clone()
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        self.prompt_path.clone().map(Cow::Owned)
    }

    fn default_permission(&self) -> ToolPolicy {
        self.default_permission.clone()
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        self.allow_patterns
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        self.deny_patterns
    }
}

struct RegistryFnTool {
    registry: ToolRegistry,
    executor: ToolExecutorFn,
    metadata: RegistrationMetadataSnapshot,
}

impl RegistryFnTool {
    fn from_registration(registry: ToolRegistry, registration: &ToolRegistration) -> Option<Self> {
        let executor = match registration.handler() {
            ToolHandler::RegistryFn(executor) => executor,
            ToolHandler::TraitObject(_) => return None,
        };

        Some(Self {
            registry,
            executor,
            metadata: RegistrationMetadataSnapshot::from_registration(registration),
        })
    }
}

#[async_trait]
impl Tool for RegistryFnTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        (self.executor)(&self.registry, args).await
    }

    fn name(&self) -> &str {
        self.metadata.name()
    }

    fn description(&self) -> &str {
        self.metadata.description.as_ref()
    }

    fn parameter_schema(&self) -> Option<Value> {
        self.metadata.parameter_schema.clone()
    }

    fn config_schema(&self) -> Option<Value> {
        self.metadata.config_schema()
    }

    fn state_schema(&self) -> Option<Value> {
        self.metadata.state_schema()
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        self.metadata.prompt_path()
    }

    fn default_permission(&self) -> ToolPolicy {
        self.metadata.default_permission()
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        self.metadata.allow_patterns()
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        self.metadata.deny_patterns()
    }
}

struct RegistrationBackedTool<T> {
    inner: T,
    metadata: RegistrationMetadataSnapshot,
}

impl<T> RegistrationBackedTool<T>
where
    T: Tool + Send + Sync,
{
    fn from_registration(inner: T, registration: &ToolRegistration) -> Self {
        let metadata = RegistrationMetadataSnapshot::from_registration_with_tool(registration, &inner);
        Self { inner, metadata }
    }
}

#[async_trait]
impl<T> Tool for RegistrationBackedTool<T>
where
    T: Tool + Send + Sync,
{
    async fn execute(&self, args: Value) -> Result<Value> {
        self.inner.execute(args).await
    }

    async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
        let mut result = self.inner.execute_dual(args).await?;
        result.tool_name = self.name().to_string();
        Ok(result)
    }

    fn name(&self) -> &str {
        self.metadata.name()
    }

    fn description(&self) -> &str {
        if self.metadata.name.as_ref() == crate::config::constants::tools::TASK_TRACKER {
            self.inner.description()
        } else {
            self.metadata.description.as_ref()
        }
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        self.inner.validate_args(args)
    }

    fn parameter_schema(&self) -> Option<Value> {
        if self.metadata.name.as_ref() == crate::config::constants::tools::TASK_TRACKER {
            self.inner.parameter_schema()
        } else {
            self.metadata.parameter_schema.clone()
        }
    }

    fn config_schema(&self) -> Option<Value> {
        self.metadata.config_schema()
    }

    fn state_schema(&self) -> Option<Value> {
        self.metadata.state_schema()
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        self.metadata.prompt_path()
    }

    fn default_permission(&self) -> ToolPolicy {
        self.metadata.default_permission()
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        self.metadata.allow_patterns()
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        self.metadata.deny_patterns()
    }

    fn is_mutating(&self) -> bool {
        self.inner.is_mutating()
    }

    fn is_parallel_safe(&self) -> bool {
        self.inner.is_parallel_safe()
    }

    fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    fn resource_hints(&self, args: &Value) -> Vec<String> {
        self.inner.resource_hints(args)
    }

    fn execution_cost(&self) -> u8 {
        self.inner.execution_cost()
    }
}

struct RegistrationBackedDynTool {
    inner: Arc<dyn Tool>,
    metadata: RegistrationMetadataSnapshot,
}

impl RegistrationBackedDynTool {
    fn from_registration(inner: Arc<dyn Tool>, registration: &ToolRegistration) -> Self {
        let metadata = RegistrationMetadataSnapshot::from_registration_with_tool(registration, inner.as_ref());
        Self { inner, metadata }
    }
}

#[async_trait]
impl Tool for RegistrationBackedDynTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        self.inner.execute(args).await
    }

    async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
        let mut result = self.inner.execute_dual(args).await?;
        result.tool_name = self.name().to_string();
        Ok(result)
    }

    fn name(&self) -> &str {
        self.metadata.name()
    }

    fn description(&self) -> &str {
        if self.metadata.name.as_ref() == crate::config::constants::tools::TASK_TRACKER {
            self.inner.description()
        } else {
            self.metadata.description.as_ref()
        }
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        self.inner.validate_args(args)
    }

    fn parameter_schema(&self) -> Option<Value> {
        if self.metadata.name.as_ref() == crate::config::constants::tools::TASK_TRACKER {
            self.inner.parameter_schema()
        } else {
            self.metadata.parameter_schema.clone()
        }
    }

    fn config_schema(&self) -> Option<Value> {
        self.metadata.config_schema()
    }

    fn state_schema(&self) -> Option<Value> {
        self.metadata.state_schema()
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        self.metadata.prompt_path()
    }

    fn default_permission(&self) -> ToolPolicy {
        self.metadata.default_permission()
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        self.metadata.allow_patterns()
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        self.metadata.deny_patterns()
    }

    fn is_mutating(&self) -> bool {
        self.inner.is_mutating()
    }

    fn is_parallel_safe(&self) -> bool {
        self.inner.is_parallel_safe()
    }

    fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    fn resource_hints(&self, args: &Value) -> Vec<String> {
        self.inner.resource_hints(args)
    }

    fn execution_cost(&self) -> u8 {
        self.inner.execution_cost()
    }
}

/// Fallback bridge for registrations that already carry shared tool ownership.
///
/// Prefer `wrap_registered_native_tool()` when the caller still owns the
/// concrete tool instance and does not need to preserve an existing
/// `Arc<dyn Tool>` handle.
fn wrap_registered_trait_object_tool(
    registration: &ToolRegistration,
    tool: Arc<dyn Tool>,
    workspace_root: PathBuf,
    mode: CgpRuntimeMode,
) -> Arc<dyn Tool> {
    let tool: Arc<dyn Tool> = Arc::new(RegistrationBackedDynTool::from_registration(tool, registration));
    match mode {
        CgpRuntimeMode::Interactive => Arc::new(wrap_tool_interactive(tool, workspace_root)),
        CgpRuntimeMode::Ci => Arc::new(wrap_tool_ci(tool, workspace_root)),
    }
}

pub fn wrap_registered_native_tool<T>(
    registration: &ToolRegistration,
    tool: T,
    workspace_root: PathBuf,
    mode: CgpRuntimeMode,
) -> Arc<dyn Tool>
where
    T: Tool + Send + Sync + 'static,
{
    let tool = RegistrationBackedTool::from_registration(tool, registration);
    match mode {
        CgpRuntimeMode::Interactive => Arc::new(wrap_native_tool_interactive(tool, workspace_root)),
        CgpRuntimeMode::Ci => Arc::new(wrap_native_tool_ci(tool, workspace_root)),
    }
}

pub fn native_cgp_tool_factory<T, F>(build_tool: F) -> super::registration::NativeCgpToolFactory
where
    T: Tool + Send + Sync + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    Arc::new(move |registration, workspace_root, mode| {
        wrap_registered_native_tool(registration, build_tool(), workspace_root, mode)
    })
}

/// Runtime mode for CGP pipeline selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgpRuntimeMode {
    /// Interactive TUI sessions: prompt approval + workspace sandbox + tracing.
    Interactive,
    /// CI/automation: auto-approval + strict sandbox + no middleware.
    Ci,
}

impl ToolRegistry {
    pub(crate) fn current_cgp_mode(&self) -> Option<CgpRuntimeMode> {
        *self.cgp_runtime_mode.read().unwrap_or_else(|e| e.into_inner())
    }

    fn set_cgp_runtime_mode(&self, mode: CgpRuntimeMode) {
        if let Ok(mut current_mode) = self.cgp_runtime_mode.write() {
            *current_mode = Some(mode);
        }
    }

    pub(crate) fn cgp_handler_for_registration(
        &self,
        registration: &ToolRegistration,
        mode: CgpRuntimeMode,
    ) -> Option<ToolHandler> {
        let workspace = self.workspace_root_owned();
        if let Some(factory) = registration.native_cgp_factory() {
            return Some(ToolHandler::TraitObject(factory(registration, workspace, mode)));
        }

        match registration.handler() {
            ToolHandler::TraitObject(tool) => {
                Some(ToolHandler::TraitObject(wrap_registered_trait_object_tool(registration, tool, workspace, mode)))
            }
            ToolHandler::RegistryFn(_) => {
                let tool = RegistryFnTool::from_registration(self.clone(), registration)?;
                Some(ToolHandler::TraitObject(match mode {
                    CgpRuntimeMode::Interactive => Arc::new(wrap_native_tool_interactive(tool, workspace)),
                    CgpRuntimeMode::Ci => Arc::new(wrap_native_tool_ci(tool, workspace)),
                }))
            }
        }
    }

    /// Enable the CGP pipeline for all registered tools.
    ///
    /// This replaces each eligible tool's handler with a CGP `ToolFacade`
    /// determined by the runtime mode. Registrations that provide a native CGP
    /// factory use that directly; `TraitObject` handlers are wrapped with
    /// registration-backed metadata before entering the passthrough bridge, and
    /// `RegistryFn` handlers are projected through a concrete `RegistryFnTool`.
    pub async fn enable_cgp_pipeline(&self, mode: CgpRuntimeMode) {
        self.set_cgp_runtime_mode(mode);
        let snapshot = self.inventory.registrations_snapshot();
        let mut wrapped_count = 0u32;

        for reg in &snapshot {
            if reg.is_cgp_wrapped() {
                continue;
            }

            let Some(handler) = self.cgp_handler_for_registration(reg, mode) else {
                continue;
            };

            if let Err(err) = self.inventory.replace_tool_handler(reg.name(), handler) {
                tracing::warn!(
                    tool = %reg.name(),
                    %err,
                    "Failed to wrap tool with CGP pipeline"
                );
            } else {
                wrapped_count += 1;
            }
        }

        if wrapped_count > 0 {
            self.rebuild_tool_assembly().await;
            self.tool_catalog_state.note_explicit_refresh("cgp_pipeline_enable");
            self.invalidate_hot_cache();
            tracing::info!(
                count = wrapped_count,
                mode = ?mode,
                "CGP pipeline enabled for registered tools"
            );
        }
    }

    /// Wrap a single tool through the CGP pipeline and register it.
    ///
    /// This is the preferred path for new tool registrations that should
    /// participate in the CGP approval/sandbox/logging/cache/retry pipeline.
    pub async fn register_cgp_tool(
        &self,
        tool: Arc<dyn Tool>,
        capability: crate::config::types::CapabilityLevel,
        mode: CgpRuntimeMode,
    ) -> Result<()> {
        let workspace = self.workspace_root_owned();
        let tool_name = Arc::<str>::from(tool.name());
        let registration = match mode {
            CgpRuntimeMode::Interactive => {
                ToolRegistration::from_cgp_tool(tool_name, capability, wrap_tool_interactive(tool, workspace))
            }
            CgpRuntimeMode::Ci => ToolRegistration::from_cgp_tool(tool_name, capability, wrap_tool_ci(tool, workspace)),
        };
        self.register_tool(registration).await
    }

    /// Invalidate the hot tool cache after CGP wrapping.
    fn invalidate_hot_cache(&self) {
        self.hot_tool_cache.write().clear();
        *self.cached_available_tools.write() = None;
    }
}

#[cfg(test)]
mod tests;
