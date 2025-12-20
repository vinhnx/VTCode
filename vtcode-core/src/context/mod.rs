//! Context management for vibe coding support
//!
//! This module provides intelligent context gathering and entity resolution
//! to support lazy/vague user requests. It enables "vibe coding" by inferring
//! user intent from minimal input.
//!
//! ## Components
//!
//! - `entity_resolver`: Maps vague terms to workspace entities
//! - `workspace_state`: Tracks file activity and value changes
//! - `conversation_memory`: Maintains entity mentions across conversation
//! - `proactive_gatherer`: Automatically gathers relevant context

pub mod entity_resolver;
pub mod workspace_state;
pub mod conversation_memory;
pub mod proactive_gatherer;

// Re-export key types for convenience
pub use entity_resolver::{EntityResolver, EntityIndex, EntityMatch, FileLocation};
pub use workspace_state::{WorkspaceState, FileActivity, ValueHistory};
pub use conversation_memory::{ConversationMemory, MentionHistory, EntityMention};
pub use proactive_gatherer::{ProactiveGatherer, GatheredContext};
