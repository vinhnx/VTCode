use rustc_hash::FxHashMap;

use crate::tools::handlers::{SessionToolCatalog, ToolCallError, ToolCatalogEntry};

use super::{ToolMetadata, ToolRegistration};

pub(super) struct ToolAssembly {
    policy_seeds: Vec<(String, ToolMetadata)>,
    catalog: SessionToolCatalog,
    public_routes: FxHashMap<String, String>,
    catalog_entries: FxHashMap<String, usize>,
}

impl ToolAssembly {
    pub(super) fn empty() -> Self {
        Self::from_registrations(Vec::new())
    }

    pub(super) fn from_registrations(registrations: Vec<ToolRegistration>) -> Self {
        let policy_seeds = registrations
            .iter()
            .map(|registration| {
                (
                    registration.name().to_string(),
                    registration.metadata().clone(),
                )
            })
            .collect();
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let (public_routes, catalog_entries) = build_public_routes(&catalog);
        Self {
            policy_seeds,
            catalog,
            public_routes,
            catalog_entries,
        }
    }

    pub(super) fn policy_seeds(&self) -> &[(String, ToolMetadata)] {
        &self.policy_seeds
    }

    pub(super) fn catalog(&self) -> &SessionToolCatalog {
        &self.catalog
    }

    pub(super) fn find_catalog_entry(&self, name: &str) -> Option<&ToolCatalogEntry> {
        let registration_name = self.resolve_registration_name(name).ok()?;
        let entry_index = self.catalog_entries.get(registration_name)?;
        self.catalog.entries().get(*entry_index)
    }

    pub(super) fn resolve_registration_name(&self, name: &str) -> Result<&str, ToolCallError> {
        self.public_routes
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| ToolCallError::respond(format!("Unknown tool: {name}")))
    }
}

fn build_public_routes(
    catalog: &SessionToolCatalog,
) -> (FxHashMap<String, String>, FxHashMap<String, usize>) {
    let mut public_routes = FxHashMap::default();
    let mut catalog_entries = FxHashMap::default();

    for (index, entry) in catalog.entries().iter().enumerate() {
        catalog_entries.insert(entry.registration_name.clone(), index);
        public_routes.insert(entry.public_name.clone(), entry.registration_name.clone());
        for alias in &entry.aliases {
            public_routes.insert(alias.clone(), entry.registration_name.clone());
        }
    }

    (public_routes, catalog_entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use crate::config::types::CapabilityLevel;
    use crate::tool_policy::ToolPolicy;
    use crate::tools::registry::ToolRegistration;
    use futures::future::BoxFuture;
    use serde_json::{Value, json};

    fn noop_executor<'a>(
        _registry: &'a crate::tools::registry::ToolRegistry,
        _args: Value,
    ) -> BoxFuture<'a, anyhow::Result<Value>> {
        Box::pin(async move { Ok(json!({"success": true})) })
    }

    #[test]
    fn public_routes_keep_exact_aliases_only() {
        let registration = ToolRegistration::new(
            tools::UNIFIED_EXEC,
            CapabilityLevel::Bash,
            true,
            noop_executor,
        )
        .with_description("Run commands")
        .with_parameter_schema(json!({"type": "object"}))
        .with_permission(ToolPolicy::Prompt)
        .with_aliases(["exec code", tools::EXECUTE_CODE]);

        let assembly = ToolAssembly::from_registrations(vec![registration]);

        assert_eq!(
            assembly.resolve_registration_name("exec code").ok(),
            Some(tools::UNIFIED_EXEC)
        );
        assert_eq!(
            assembly.resolve_registration_name(tools::EXECUTE_CODE).ok(),
            Some(tools::UNIFIED_EXEC)
        );
        assert!(assembly.resolve_registration_name("exec_code").is_err());
    }
}
