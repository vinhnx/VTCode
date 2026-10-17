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
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use crate::tools::handlers::{PlanningWorkflowState, TaskTrackerTool};
    use crate::tools::traits::Tool;
    use futures::future::BoxFuture;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        async fn execute(&self, args: Value) -> Result<Value> {
            Ok(serde_json::json!({
                "tool_name": "dummy",
                "echoed": args,
            }))
        }

        fn name(&self) -> &str {
            "dummy_cgp_test"
        }

        fn description(&self) -> &str {
            "A dummy tool for CGP facade tests"
        }
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_wraps_tools() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let tool: Arc<dyn Tool> = Arc::new(DummyTool);
        let reg = ToolRegistration::from_tool("dummy_cgp_test", crate::config::types::CapabilityLevel::Basic, tool);
        registry.register_tool(reg).await.expect("should register");

        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let wrapped = registry.get_tool("dummy_cgp_test");
        assert!(wrapped.is_some(), "tool should still be accessible");

        let result = wrapped
            .unwrap()
            .execute(serde_json::json!({"test": true}))
            .await
            .expect("should execute");
        assert_eq!(result.get("echoed").and_then(|v| v.get("test")), Some(&serde_json::json!(true)));
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_preserves_registration_metadata_for_trait_object_tools() {
        struct BridgeTool;

        #[async_trait]
        impl Tool for BridgeTool {
            async fn execute(&self, _args: Value) -> Result<Value> {
                Ok(serde_json::json!({ "path": "bridge" }))
            }

            async fn execute_dual(&self, _args: Value) -> Result<SplitToolResult> {
                Ok(SplitToolResult::simple(self.name(), "dual bridge"))
            }

            fn name(&self) -> &str {
                "bridge_trait_object"
            }

            fn description(&self) -> &str {
                "bridge fallback tool"
            }
        }

        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let registration = ToolRegistration::from_tool_with_metadata(
            "registered_trait_object_cgp_test",
            crate::config::types::CapabilityLevel::Basic,
            Arc::new(BridgeTool),
            crate::tools::registry::ToolMetadata::default()
                .with_description("registered trait-object tool")
                .with_parameter_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    }
                }))
                .with_prompt_path("tools/registered_trait_object.md")
                .with_permission(ToolPolicy::Allow)
                .with_allowlist(["tool://allowed"])
                .with_denylist(["tool://blocked"]),
        );
        registry.register_tool(registration).await.expect("should register");

        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let tool = registry
            .get_tool("registered_trait_object_cgp_test")
            .expect("tool should exist");
        assert_eq!(tool.name(), "registered_trait_object_cgp_test");
        assert_eq!(tool.description(), "registered trait-object tool");
        assert_eq!(tool.prompt_path().as_deref(), Some("tools/registered_trait_object.md"));
        assert_eq!(tool.default_permission(), ToolPolicy::Allow);
        assert_eq!(
            tool.parameter_schema(),
            Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }))
        );
        assert_eq!(tool.allow_patterns(), Some(&["tool://allowed"][..]));
        assert_eq!(tool.deny_patterns(), Some(&["tool://blocked"][..]));

        let dual = tool
            .execute_dual(serde_json::json!({ "query": "rust" }))
            .await
            .expect("should execute dual");
        assert_eq!(dual.tool_name, "registered_trait_object_cgp_test");
    }

    #[tokio::test]
    async fn registration_wrappers_share_snapshot_metadata_accessors() {
        // The seven snapshot-sourced accessors are byte-identical across all
        // three wrapper types; only description()/parameter_schema() carry a
        // task_tracker live-metadata branch. Wrap one non-task_tracker tool
        // all three ways and require identical output, so a future
        // per-wrapper edit cannot silently diverge one copy. Asymmetric
        // controls: the inner tool disagrees with its registration on every
        // accessor (proving the snapshot wins), and a TraitObject
        // registration is rejected by RegistryFnTool (proving why it has no
        // live branch to share).
        struct MetaTool;

        #[async_trait]
        impl Tool for MetaTool {
            async fn execute(&self, _args: Value) -> Result<Value> {
                Ok(Value::Null)
            }

            fn name(&self) -> &str {
                "meta_probe_inner"
            }

            fn description(&self) -> &str {
                "inner description"
            }
        }

        fn echo_executor<'a>(_registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
            Box::pin(async move { Ok(args) })
        }

        let parameter_schema = serde_json::json!({
            "type": "object",
            "properties": { "query": { "type": "string" } },
        });
        let config_schema = serde_json::json!({ "type": "object" });
        let state_schema = serde_json::json!({ "type": "object" });
        let registration = ToolRegistration::from_tool_with_metadata(
            "meta_probe",
            crate::config::types::CapabilityLevel::Basic,
            Arc::new(MetaTool),
            crate::tools::registry::ToolMetadata::default()
                .with_description("registered description")
                .with_parameter_schema(parameter_schema.clone())
                .with_config_schema(config_schema.clone())
                .with_state_schema(state_schema.clone())
                .with_prompt_path("tools/meta_probe.md")
                .with_permission(ToolPolicy::Allow)
                .with_allowlist(["tool://allowed"])
                .with_denylist(["tool://blocked"]),
        );

        let snapshot = RegistrationMetadataSnapshot::from_registration(&registration);
        assert_eq!(snapshot.name(), "meta_probe");
        assert_eq!(snapshot.config_schema(), Some(config_schema.clone()));
        assert_eq!(snapshot.state_schema(), Some(state_schema.clone()));
        assert_eq!(snapshot.prompt_path().as_deref(), Some("tools/meta_probe.md"));
        assert_eq!(snapshot.default_permission(), ToolPolicy::Allow);
        assert_eq!(snapshot.allow_patterns(), Some(&["tool://allowed"][..]));
        assert_eq!(snapshot.deny_patterns(), Some(&["tool://blocked"][..]));

        let backed = RegistrationBackedTool::from_registration(MetaTool, &registration);
        let backed_dyn = RegistrationBackedDynTool::from_registration(Arc::new(MetaTool), &registration);
        for (label, tool) in [
            ("backed", &backed as &dyn Tool),
            ("backed_dyn", &backed_dyn as &dyn Tool),
        ] {
            assert_eq!(tool.name(), snapshot.name(), "{label} name");
            assert_eq!(tool.description(), "registered description", "{label} keeps snapshot over inner");
            assert_eq!(tool.parameter_schema(), Some(parameter_schema.clone()), "{label} parameter schema");
            assert_eq!(tool.config_schema(), snapshot.config_schema(), "{label} config schema");
            assert_eq!(tool.state_schema(), snapshot.state_schema(), "{label} state schema");
            assert_eq!(tool.prompt_path().as_deref(), Some("tools/meta_probe.md"), "{label} prompt path");
            assert_eq!(tool.default_permission(), ToolPolicy::Allow, "{label} permission");
            assert_eq!(tool.allow_patterns(), Some(&["tool://allowed"][..]), "{label} allowlist");
            assert_eq!(tool.deny_patterns(), Some(&["tool://blocked"][..]), "{label} denylist");
        }

        let temp_dir = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let fn_registration =
            ToolRegistration::new("meta_probe_fn", crate::config::types::CapabilityLevel::Basic, false, echo_executor);
        let func = RegistryFnTool::from_registration(registry, &fn_registration)
            .expect("RegistryFn handler registration must wrap");
        let fn_snapshot = RegistrationMetadataSnapshot::from_registration(&fn_registration);
        assert_eq!(func.name(), fn_snapshot.name());
        assert_eq!(func.description(), "");
        assert_eq!(func.parameter_schema(), None);
        assert_eq!(func.config_schema(), fn_snapshot.config_schema());
        assert_eq!(func.state_schema(), fn_snapshot.state_schema());
        assert_eq!(func.prompt_path().as_deref(), None);
        assert_eq!(func.default_permission(), ToolPolicy::Prompt);
        assert_eq!(func.allow_patterns(), None);
        assert_eq!(func.deny_patterns(), None);

        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        assert!(
            RegistryFnTool::from_registration(registry, &registration).is_none(),
            "TraitObject registrations must not wrap as RegistryFnTool"
        );
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_ci_mode() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let tool: Arc<dyn Tool> = Arc::new(DummyTool);
        let reg = ToolRegistration::from_tool("dummy_cgp_test", crate::config::types::CapabilityLevel::Basic, tool);
        registry.register_tool(reg).await.expect("should register");

        registry.enable_cgp_pipeline(CgpRuntimeMode::Ci).await;

        let wrapped = registry.get_tool("dummy_cgp_test");
        assert!(wrapped.is_some());

        let result = wrapped
            .unwrap()
            .execute(serde_json::json!({"ci": "mode"}))
            .await
            .expect("should execute");
        assert_eq!(result.get("echoed").and_then(|v| v.get("ci")).and_then(|v| v.as_str()), Some("mode"));
    }

    #[tokio::test]
    async fn register_cgp_tool_directly() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let tool: Arc<dyn Tool> = Arc::new(DummyTool);

        registry
            .register_cgp_tool(tool, crate::config::types::CapabilityLevel::Basic, CgpRuntimeMode::Interactive)
            .await
            .expect("should register");

        let wrapped = registry.get_tool("dummy_cgp_test");
        assert!(wrapped.is_some());
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_skips_already_wrapped_tools() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let tool: Arc<dyn Tool> = Arc::new(DummyTool);

        registry
            .register_cgp_tool(tool, crate::config::types::CapabilityLevel::Basic, CgpRuntimeMode::Interactive)
            .await
            .expect("should register");

        let before = registry
            .inventory
            .registrations_snapshot()
            .into_iter()
            .find(|registration| registration.name() == "dummy_cgp_test")
            .expect("registration should exist");
        assert!(before.is_cgp_wrapped());

        let before_handler = match before.handler() {
            ToolHandler::TraitObject(tool) => tool,
            ToolHandler::RegistryFn(_) => panic!("expected trait object handler"),
        };

        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let after = registry
            .inventory
            .registrations_snapshot()
            .into_iter()
            .find(|registration| registration.name() == "dummy_cgp_test")
            .expect("registration should exist");
        let after_handler = match after.handler() {
            ToolHandler::TraitObject(tool) => tool,
            ToolHandler::RegistryFn(_) => panic!("expected trait object handler"),
        };

        assert!(Arc::ptr_eq(&before_handler, &after_handler));
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_prefers_native_cgp_factory() {
        struct BridgeTool;

        #[async_trait]
        impl Tool for BridgeTool {
            async fn execute(&self, _args: Value) -> Result<Value> {
                Ok(serde_json::json!({ "path": "bridge" }))
            }

            fn name(&self) -> &str {
                "native_cgp_factory_test"
            }

            fn description(&self) -> &str {
                "bridge fallback tool"
            }
        }

        struct NativeTool;

        #[async_trait]
        impl Tool for NativeTool {
            async fn execute(&self, _args: Value) -> Result<Value> {
                Ok(serde_json::json!({ "path": "native" }))
            }

            fn name(&self) -> &str {
                "native_cgp_factory_test"
            }

            fn description(&self) -> &str {
                "native factory tool"
            }
        }

        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let reg = ToolRegistration::from_tool(
            "native_cgp_factory_test",
            crate::config::types::CapabilityLevel::Basic,
            Arc::new(BridgeTool),
        )
        .with_description("registered native factory tool")
        .with_native_cgp_factory(Arc::new(|registration, workspace_root, mode| {
            wrap_registered_native_tool(registration, NativeTool, workspace_root, mode)
        }));
        registry.register_tool(reg).await.expect("should register");

        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let tool = registry.get_tool("native_cgp_factory_test").expect("tool should exist");
        assert_eq!(tool.name(), "native_cgp_factory_test");
        assert_eq!(tool.description(), "registered native factory tool");

        let result = tool.execute(serde_json::json!({})).await.expect("should execute");

        assert_eq!(result.get("path").and_then(|v| v.as_str()), Some("native"));
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_uses_live_task_tracker_metadata_for_native_factory_tools() {
        let temp_dir = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let plan_state = registry.planning_workflow_state();
        let registration = registry
            .inventory
            .get_registration(tools::TASK_TRACKER)
            .expect("task_tracker registration exists");

        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let tool = registry.get_tool(tools::TASK_TRACKER).expect("wrapped task_tracker tool");
        assert_eq!(
            tool.description(),
            crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(false)
        );
        assert_eq!(tool.parameter_schema().expect("standard schema")["properties"]["index"]["minimum"], 0);

        plan_state.enable();

        let tool = registry.get_tool(tools::TASK_TRACKER).expect("wrapped task_tracker tool");
        assert_eq!(
            tool.description(),
            crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(true)
        );
        assert_eq!(tool.parameter_schema().expect("planning schema")["properties"]["index"]["minimum"], 1);

        let direct_native = wrap_registered_native_tool(
            &registration,
            TaskTrackerTool::new(temp_dir.path().to_path_buf(), plan_state.clone()),
            temp_dir.path().to_path_buf(),
            CgpRuntimeMode::Interactive,
        );
        assert_eq!(
            direct_native.description(),
            crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(true)
        );
        assert_eq!(
            direct_native.parameter_schema().expect("native planning schema")["properties"]["index"]["minimum"],
            1
        );
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_uses_live_task_tracker_metadata_for_trait_object_tools() {
        let temp_dir = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let registration = ToolRegistration::from_tool(
            tools::TASK_TRACKER,
            crate::config::types::CapabilityLevel::Basic,
            Arc::new(TaskTrackerTool::new(temp_dir.path().to_path_buf(), state.clone())),
        )
        .with_description("stale registered task tracker metadata");

        registry.register_tool(registration).await.expect("should register override");
        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let tool = registry.get_tool(tools::TASK_TRACKER).expect("wrapped task_tracker tool");
        assert_eq!(
            tool.description(),
            crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(false)
        );

        state.enable();

        let tool = registry.get_tool(tools::TASK_TRACKER).expect("wrapped task_tracker tool");
        assert_eq!(
            tool.description(),
            crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(true)
        );
        assert_eq!(tool.parameter_schema().expect("planning schema")["properties"]["index"]["minimum"], 1);
    }

    #[tokio::test]
    async fn register_tool_after_enabling_cgp_pipeline_wraps_new_tools() {
        struct LateTool;

        #[async_trait]
        impl Tool for LateTool {
            async fn execute(&self, args: Value) -> Result<Value> {
                Ok(serde_json::json!({
                    "path": "late-bridge",
                    "args": args,
                }))
            }

            fn name(&self) -> &str {
                "late_cgp_test"
            }

            fn description(&self) -> &str {
                "late registration test"
            }
        }

        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        registry
            .register_tool(ToolRegistration::from_tool(
                "late_cgp_test",
                crate::config::types::CapabilityLevel::Basic,
                Arc::new(LateTool),
            ))
            .await
            .expect("should register");

        let registration = registry
            .inventory
            .registrations_snapshot()
            .into_iter()
            .find(|registration| registration.name() == "late_cgp_test")
            .expect("registration should exist");
        assert!(registration.is_cgp_wrapped());

        let result = registry
            .get_tool("late_cgp_test")
            .expect("tool should exist")
            .execute(serde_json::json!({"late": true}))
            .await
            .expect("should execute");
        assert_eq!(result.get("path").and_then(|v| v.as_str()), Some("late-bridge"));
    }

    #[tokio::test]
    async fn register_tool_after_enabling_cgp_pipeline_prefers_native_factory() {
        struct BridgeTool;

        #[async_trait]
        impl Tool for BridgeTool {
            async fn execute(&self, _args: Value) -> Result<Value> {
                Ok(serde_json::json!({ "path": "bridge" }))
            }

            fn name(&self) -> &str {
                "late_native_cgp_test"
            }

            fn description(&self) -> &str {
                "bridge fallback tool"
            }
        }

        struct NativeTool;

        #[async_trait]
        impl Tool for NativeTool {
            async fn execute(&self, _args: Value) -> Result<Value> {
                Ok(serde_json::json!({ "path": "late-native" }))
            }

            fn name(&self) -> &str {
                "late_native_cgp_test"
            }

            fn description(&self) -> &str {
                "native late tool"
            }
        }

        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let registration = ToolRegistration::from_tool(
            "late_native_cgp_test",
            crate::config::types::CapabilityLevel::Basic,
            Arc::new(BridgeTool),
        )
        .with_native_cgp_factory(Arc::new(|registration, workspace_root, mode| {
            wrap_registered_native_tool(registration, NativeTool, workspace_root, mode)
        }));
        registry.register_tool(registration).await.expect("should register");

        let result = registry
            .get_tool("late_native_cgp_test")
            .expect("tool should exist")
            .execute(serde_json::json!({}))
            .await
            .expect("should execute");

        assert_eq!(result.get("path").and_then(|v| v.as_str()), Some("late-native"));
    }

    fn registry_fn_test_executor<'a>(_registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            Ok(serde_json::json!({
                "tool_name": "registry_fn_cgp_test",
                "echoed": args,
            }))
        })
    }

    #[tokio::test]
    async fn enable_cgp_pipeline_wraps_registry_fn_tools() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        let registration = ToolRegistration::new(
            "registry_fn_cgp_test",
            crate::config::types::CapabilityLevel::Basic,
            false,
            registry_fn_test_executor,
        )
        .with_description("Registry function CGP test tool")
        .with_parameter_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "flag": { "type": "boolean" }
            }
        }))
        .with_permission(ToolPolicy::Allow);
        registry.register_tool(registration).await.expect("should register");

        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        let wrapped = registry.get_tool("registry_fn_cgp_test").expect("tool exists");
        assert_eq!(wrapped.name(), "registry_fn_cgp_test");
        assert_eq!(wrapped.description(), "Registry function CGP test tool");
        assert!(wrapped.parameter_schema().is_some());
        assert_eq!(wrapped.default_permission(), ToolPolicy::Allow);

        let result = wrapped
            .execute(serde_json::json!({"flag": true}))
            .await
            .expect("should execute");
        assert_eq!(
            result
                .get("echoed")
                .and_then(|value| value.get("flag"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn register_registry_fn_after_enabling_cgp_pipeline_wraps_new_tools() {
        let registry = ToolRegistry::new(PathBuf::from("/tmp/test")).await;
        registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

        registry
            .register_tool(ToolRegistration::new(
                "late_registry_fn_cgp_test",
                crate::config::types::CapabilityLevel::Basic,
                false,
                registry_fn_test_executor,
            ))
            .await
            .expect("should register");

        let registration = registry
            .inventory
            .registrations_snapshot()
            .into_iter()
            .find(|registration| registration.name() == "late_registry_fn_cgp_test")
            .expect("registration should exist");
        assert!(registration.is_cgp_wrapped());

        let result = registry
            .get_tool("late_registry_fn_cgp_test")
            .expect("tool exists")
            .execute(serde_json::json!({"late": true}))
            .await
            .expect("should execute");
        assert_eq!(
            result
                .get("echoed")
                .and_then(|value| value.get("late"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }
}
