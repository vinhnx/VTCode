//! Native composer resource-discovery DTOs.

use serde::{Deserialize, Serialize};

/// A discoverable source for the native composer's Add menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComposerResourceKind {
    /// A CLI skill with an explicit dollar-name mention.
    Skill,
    /// An installed Agent Plugin.
    Plugin,
}

/// Bounded, non-secret discovery metadata; never executable content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerResource {
    /// Validated resource name.
    pub name: String,
    /// Short plain-text description.
    pub description: String,
    /// Which shared CLI discovery source owns this entry.
    pub kind: ComposerResourceKind,
}

impl ComposerResource {
    /// Resource names are safe single tokens for explicit CLI skill mentions.
    pub fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 128
            && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    }
}
