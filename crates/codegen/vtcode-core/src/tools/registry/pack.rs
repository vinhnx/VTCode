//! ToolPack trait and builtin pack implementations.
//!
//! A `ToolPack` groups related tools with shared lifecycle hooks, replacing
//! the flat `#[distributed_slice]` factory pattern for builtin tools. Packs
//! enable logical grouping, capability negotiation, and batched registration
//! with fewer lock acquisitions.
//!
//! ## Design
//!
//! ```text
//! ToolPack (trait)
//!   ├── pack_id()        - unique identifier (e.g. "shell", "web")
//!   ├── register()       - batch-register tools into the inventory
//!   └── configure()      - optional config hook
//! ```
//!
//! Builtin packs are collected via `linkme::distributed_slice` into
//! `BUILTIN_PACKS`, then iterated at startup by `register_builtin_packs()`.
//! Each pack's `register()` method groups its tool registrations so behavior
//! and catalog-source decoration stays in one place.

use crate::tools::handlers::PlanningWorkflowState;
use crate::tools::registry::distributed::ToolConfigSnapshot;
use crate::tools::registry::inventory::ToolInventory;
use crate::tools::registry::registration::{ToolCatalogSource, ToolRegistration};
use crate::tools::tool_intent::builtin_tool_behavior;

/// A logical grouping of related tools with shared lifecycle hooks.
///
/// Packs are collected at link time via `#[distributed_slice(BUILTIN_PACKS)]`
/// and iterated during `ToolRegistry` construction. Each pack is responsible
/// for batch-registering its tools into the inventory.
///
/// The pack receives an immutable workspace tool-config snapshot explicitly;
/// pack construction must not read process-global workspace state.
#[async_trait::async_trait]
pub trait ToolPack: Send + Sync {
    /// Unique identifier for this pack (e.g. "shell", "web", "planning").
    fn pack_id(&self) -> &'static str;

    /// Register all tools in this pack into the inventory.
    ///
    /// Implementations should batch registrations where possible.
    async fn register(
        &self,
        inventory: &ToolInventory,
        plan_state: &PlanningWorkflowState,
        tool_config: &ToolConfigSnapshot,
    );
}

/// Batch-register a list of tools into the inventory, logging any failures.
///
/// This is the preferred registration path for packs: it centralizes the
/// built-in behavior and catalog-source decoration while preserving
/// per-registration validation and failure isolation.
pub fn batch_register(inventory: &ToolInventory, registrations: Vec<ToolRegistration>) {
    for mut registration in registrations {
        let tool_name = registration.name().to_string();
        if let Some(behavior) = builtin_tool_behavior(&tool_name) {
            registration = registration.with_behavior(behavior);
        }
        registration = registration.with_catalog_source(ToolCatalogSource::Builtin);
        if let Err(err) = inventory.register_tool(registration) {
            tracing::warn!(tool = %tool_name, %err, "Failed to register tool from pack");
        }
    }
}

// ===========================================================================
// Pack factory type
// ===========================================================================

/// Factory function type for builtin packs.
///
/// Each pack module defines a function matching this signature and annotates
/// it with `#[distributed_slice(BUILTIN_PACKS)]`.
pub type BuiltinPackFactory = fn() -> Box<dyn ToolPack>;

/// Distributed slice of builtin pack factory functions.
///
/// Elements are placed by `#[distributed_slice(BUILTIN_PACKS)]` annotations.
/// The linker collects them into a contiguous `&'static [BuiltinPackFactory]`.
#[linkme::distributed_slice]
pub static BUILTIN_PACKS: [BuiltinPackFactory] = [..];

/// Iterate all builtin packs and invoke their `register` methods.
///
/// This is the new entry point for builtin tool registration, replacing
/// the old `builtin_tool_registrations()` loop over individual factories.
/// Packs use a deterministic model-facing priority for the core execution
/// surface, then fall back to `pack_id` so catalog projections share one
/// stable order.
pub async fn register_builtin_packs(
    inventory: &ToolInventory,
    plan_state: &PlanningWorkflowState,
    tool_config: &ToolConfigSnapshot,
) {
    let mut packs: Vec<Box<dyn ToolPack>> = BUILTIN_PACKS.iter().map(|factory| factory()).collect();

    packs.sort_by_key(|pack| (pack_priority(pack.pack_id()), pack.pack_id()));

    for pack in packs {
        pack.register(inventory, plan_state, tool_config).await;
    }
}

fn pack_priority(pack_id: &str) -> usize {
    match pack_id {
        // Keep the compact interactive surface in the same order used by
        // public schemas and declarations: edit, execute, discover, stdin.
        "editing" => 0,
        "shell" => 1,
        "search" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use std::sync::Arc;

    #[test]
    fn builtin_packs_slice_is_populated() {
        assert!(BUILTIN_PACKS.len() >= 8, "expected at least 8 builtin packs, found {}", BUILTIN_PACKS.len());
    }

    #[test]
    fn pack_ids_are_unique_and_sorted() {
        let mut ids: Vec<&'static str> = BUILTIN_PACKS.iter().map(|f| f().pack_id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BUILTIN_PACKS.len(), "pack_ids must be unique");
    }

    #[tokio::test]
    async fn register_builtin_packs_populates_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let inventory = ToolInventory::new(
            temp.path().to_path_buf(),
            Arc::new(crate::tools::edited_file_monitor::EditedFileMonitor::new()),
        );
        let plan_state = PlanningWorkflowState::new(temp.path().to_path_buf());

        register_builtin_packs(&inventory, &plan_state, &ToolConfigSnapshot::default()).await;

        assert!(inventory.has_tool(tools::CODE_SEARCH), "CODE_SEARCH should be registered");
        assert!(inventory.has_tool(tools::EXEC_COMMAND), "EXEC_COMMAND should be registered");
        assert!(inventory.has_tool(tools::SEARCH_TOOLS), "SEARCH_TOOLS should be registered");
        assert!(inventory.has_tool(tools::RECORD_DECISION), "RECORD_DECISION should be registered");
    }

    #[tokio::test]
    async fn planning_pack_task_tracker_registration_matches_planning_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let inventory = ToolInventory::new(
            temp.path().to_path_buf(),
            Arc::new(crate::tools::edited_file_monitor::EditedFileMonitor::new()),
        );
        let plan_state = PlanningWorkflowState::new(temp.path().to_path_buf());
        plan_state.enable();

        register_builtin_packs(&inventory, &plan_state, &ToolConfigSnapshot::default()).await;

        let registration = inventory
            .get_registration(tools::TASK_TRACKER)
            .expect("planning task_tracker registration should exist");
        assert_eq!(
            registration.metadata().description(),
            Some(crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(true))
        );
        assert_eq!(
            registration.metadata().parameter_schema().expect("planning schema")["properties"]["index"]["minimum"],
            1
        );
    }
}
