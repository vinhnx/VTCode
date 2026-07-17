//! Versioned state schemas: explicit schema versioning and migration for durable state.
//!
//! Following the principle of "state as a first-class citizen" (Hitchhiker's Guide
//! to Agentic AI, Section 18.6), all serialized state carries an explicit schema
//! version so that forward/backward migration is safe and auditable.
//!
//! ## Schema History
//!
//! | Version | Description |
//! |---------|-------------|
//! | v0      | Implicit — no schema version field present (legacy snapshots, pre-2025) |
//! | v1      | Initial explicit schema. Adds per-message metadata scaffolding. |
//!
//! ## Migration
//!
//! Every `VersionedState` implementor provides a `migrate()` method that walks
//! from its current version to the target, one step at a time. A snapshot at v0
//! is first migrated to v1, then to v2, and so on. This keeps each migration
//! step small and testable.

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Explicit schema version for durable agent state.
///
/// Wraps a `u32` so the version is a distinct type, not a bare integer that
/// could be confused with other version fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SchemaVersion(pub u32);

impl SchemaVersion {
    /// Implicit pre-schema state: no version field present in serialized data.
    pub const V0: SchemaVersion = SchemaVersion(0);

    /// Initial explicit schema. Adds per-message metadata placeholders.
    pub const V1: SchemaVersion = SchemaVersion(1);

    /// The current schema version that all new state should use.
    pub const CURRENT: SchemaVersion = SchemaVersion::V1;

    /// Return the inner `u32` value.
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// Trait for types whose serialized representation carries a schema version
/// and can be migrated forward to a newer version.
///
/// Implementors provide `migrate_one_step`, and the default `migrate()` method
/// walks through all intermediate versions automatically.
pub trait VersionedState: Sized + Serialize + DeserializeOwned {
    /// Return the schema version this instance was created at.
    fn schema_version(&self) -> SchemaVersion;

    /// The next schema version after `current`, if any.
    fn next_version(current: SchemaVersion) -> Option<SchemaVersion> {
        match current {
            SchemaVersion::V0 => Some(SchemaVersion::V1),
            SchemaVersion::V1 => None,
            _ => None,
        }
    }

    /// Migrate this instance to `target`, stepping through intermediate
    /// versions one at a time. Returns the fully migrated instance or an
    /// error if a migration step fails.
    ///
    /// If `target <= self.schema_version()` this is a no-op.
    fn migrate(self, target: SchemaVersion) -> Result<Self> {
        let mut current = self.schema_version();
        if current >= target {
            return Ok(self);
        }
        let mut value = self;
        loop {
            let next = Self::next_version(current);
            match next {
                Some(version) if version <= target => {
                    value = value.migrate_one_step(current, version)?;
                    current = version;
                }
                _ => break,
            }
        }
        Ok(value)
    }

    /// Perform a single version-to-version migration.
    ///
    /// Implementors should match on `(from, to)` rather than branching on
    /// `from` alone, ensuring every transition is explicitly handled.
    fn migrate_one_step(self, from: SchemaVersion, to: SchemaVersion) -> Result<Self>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that SchemaVersion constants have the expected values.
    #[test]
    fn schema_version_constants() {
        assert_eq!(SchemaVersion::V0.as_u32(), 0);
        assert_eq!(SchemaVersion::V1.as_u32(), 1);
        assert_eq!(SchemaVersion::CURRENT.as_u32(), 1);
    }

    /// Verify the next_version chain via the TestState trait implementation.
    #[test]
    fn next_version_chain() {
        assert_eq!(TestState::next_version(SchemaVersion::V0), Some(SchemaVersion::V1));
        assert_eq!(TestState::next_version(SchemaVersion::V1), None);
    }

    /// A minimal test struct to verify the VersionedState trait compiles and
    /// the migrate method runs correct steps.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestState {
        version: SchemaVersion,
        value: u32,
    }

    impl VersionedState for TestState {
        fn schema_version(&self) -> SchemaVersion {
            self.version
        }

        fn migrate_one_step(self, from: SchemaVersion, to: SchemaVersion) -> Result<Self> {
            match (from, to) {
                (SchemaVersion::V0, SchemaVersion::V1) => {
                    // v0 -> v1: set the explicit version, default new fields
                    Ok(Self { version: SchemaVersion::V1, ..self })
                }
                _ => anyhow::bail!("unsupported migration: {from:?} -> {to:?}"),
            }
        }
    }

    #[test]
    fn test_migrate_v0_to_v1() {
        let state = TestState { version: SchemaVersion::V0, value: 42 };
        let migrated = state.migrate(SchemaVersion::V1).unwrap();
        assert_eq!(migrated.version, SchemaVersion::V1);
        assert_eq!(migrated.value, 42);
    }

    #[test]
    fn test_migrate_at_target_is_noop() {
        let state = TestState { version: SchemaVersion::V1, value: 99 };
        let migrated = state.migrate(SchemaVersion::V1).unwrap();
        assert_eq!(migrated.version, SchemaVersion::V1);
        assert_eq!(migrated.value, 99);
    }

    #[test]
    fn test_migrate_above_current_is_noop() {
        let state = TestState { version: SchemaVersion::V0, value: 7 };
        // Migrate to V0 (which is <= V0) — should be a no-op
        let migrated = state.migrate(SchemaVersion::V0).unwrap();
        assert_eq!(migrated.version, SchemaVersion::V0);
        assert_eq!(migrated.value, 7);
    }
}
