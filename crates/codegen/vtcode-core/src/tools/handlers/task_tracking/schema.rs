use super::*;

pub(crate) const TASK_ITEMS_DESCRIPTION: &str = "Full checklist replacement for create or bulk update, not indexed updates. Send plain descriptions with [x]/[~]/[!]/[ ] status prefixes or actual objects with description and status; never JSON-encoded strings. Bulk updates preserve omitted metadata for unique unchanged descriptions. Explicit metadata replaces it; use empty arrays or an empty outcome to clear fields. Duplicate descriptions with existing metadata require explicit files, outcome, and verify; include scope_review when existing scope checks would otherwise be ambiguous.";

/// Shared status values for every task-tracker schema (standard + planning,
/// top-level and nested item objects). Single source so a future status
/// addition cannot drift between the planning on/off variants.
pub(crate) const TASK_STATUS_VALUES: [&str; 4] = ["pending", "in_progress", "completed", "blocked"];

/// Base `status` type schema without a description (callers add their own).
pub(crate) fn task_status_schema() -> Value {
    json!({
        "type": "string",
        "enum": TASK_STATUS_VALUES
    })
}

/// Base `files` type schema without a description.
pub(crate) fn task_files_schema() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" }
    })
}

/// Base `verify` type schema without a description (string or string list).
pub(crate) fn task_verify_schema() -> Value {
    json!({
        "anyOf": [
            { "type": "string" },
            {
                "type": "array",
                "items": { "type": "string" }
            }
        ]
    })
}

fn with_description(mut schema: Value, description: &str) -> Value {
    if let Value::Object(map) = &mut schema {
        map.insert("description".to_string(), Value::String(description.to_string()));
    }
    schema
}

/// Shared `allOf` conditional blocks for both tracker parameter schemas.
///
/// The create, update, no-indices, and add blocks are identical across
/// trackers; the update `anyOf` arms are order-normalized (validation is
/// order-independent and no test indexes arm positions). The `index == 0`
/// completion rule is standard-tracker-only (planning indices start at 1):
/// `include_index_zero_rule` inserts it at position 2, preserving the
/// `allOf[2]` contract asserted by the standard schema tests.
pub(crate) fn tracker_schema_allof(include_index_zero_rule: bool) -> Vec<Value> {
    let mut allof = vec![
        json!({
            "if": {
                "properties": { "action": { "const": "create" } },
                "required": ["action"]
            },
            "then": {
                "required": ["items"]
            }
        }),
        json!({
            "if": {
                "properties": { "action": { "const": "update" } },
                "required": ["action"]
            },
            "then": {
                "anyOf": [
                    {
                        "required": ["index", "status"],
                        "not": {
                            "anyOf": [
                                { "required": ["items"] },
                                { "required": ["index_path"] }
                            ]
                        }
                    },
                    {
                        "required": ["index_path", "status"],
                        "not": {
                            "anyOf": [
                                { "required": ["items"] },
                                { "required": ["index"] }
                            ]
                        }
                    },
                    {
                        "required": ["items"],
                        "not": {
                            "anyOf": [
                                { "required": ["index"] },
                                { "required": ["index_path"] },
                                { "required": ["status"] }
                            ]
                        }
                    }
                ]
            }
        }),
        json!({
            "if": {
                "properties": { "action": { "enum": ["create", "list", "add"] } },
                "required": ["action"]
            },
            "then": {
                "not": {
                    "anyOf": [
                        { "required": ["index"] },
                        { "required": ["index_path"] }
                    ]
                }
            }
        }),
        json!({
            "if": {
                "properties": { "action": { "const": "add" } },
                "required": ["action"]
            },
            "then": {
                "required": ["description"]
            }
        }),
    ];
    if include_index_zero_rule {
        allof.insert(
            2,
            json!({
                "if": {
                    "properties": { "index": { "const": 0 } },
                    "required": ["index"]
                },
                "then": {
                    "properties": {
                        "action": { "const": "update" },
                        "status": { "const": "completed" }
                    },
                    "required": ["action", "status"]
                }
            }),
        );
    }
    allof
}

/// Shared nested item-object schema (description + optional status/files/
/// outcome/verify). Used inside the `items` array of both tracker schemas.
pub(crate) fn task_item_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "description": { "type": "string" },
            "status": task_status_schema(),
            "files": task_files_schema(),
            "outcome": { "type": "string" },
            "verify": task_verify_schema(),
            "scope_review": with_description(task_verify_schema(), "Workspace scope checks; do not satisfy functional verification.")
        },
        "required": ["description"]
    })
}

/// Shared `items` array schema with a caller-supplied description.
pub(crate) fn task_items_array_schema(items_description: &str) -> Value {
    with_description(
        json!({
            "type": "array",
            "items": {
                "anyOf": [
                    { "type": "string" },
                    task_item_object_schema()
                ]
            }
        }),
        items_description,
    )
}

/// Top-level `status` property with its own description.
pub(crate) fn task_status_property_schema(description: &str) -> Value {
    with_description(task_status_schema(), description)
}

/// Top-level `files` property with its own description.
pub(crate) fn task_files_property_schema(description: &str) -> Value {
    with_description(task_files_schema(), description)
}

/// Top-level `verify` property with its own description.
pub(crate) fn task_verify_property_schema(description: &str) -> Value {
    with_description(task_verify_schema(), description)
}
