use schemars::{Schema, schema_for};

use super::{ThreadEvent, VersionedThreadEvent};

/// Generates a JSON Schema describing [`ThreadEvent`].
pub fn thread_event_schema() -> Schema {
    schema_for!(ThreadEvent)
}

/// Generates a JSON Schema describing [`VersionedThreadEvent`].
pub fn versioned_thread_event_schema() -> Schema {
    schema_for!(VersionedThreadEvent)
}
