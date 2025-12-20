//! Conversation memory for vibe coding support
//!
//! Tracks entities mentioned across conversation turns to enable pronoun resolution
//! and contextual understanding.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum number of conversation turns to remember
const MAX_MEMORY_TURNS: usize = 50;

/// Maximum entity mentions to track
const MAX_ENTITY_MENTIONS: usize = 200;

/// Type of entity mention
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MentionType {
    /// Direct mention (explicit naming)
    Direct,
    /// Pronoun reference (it, that, this)
    Pronoun,
    /// Implicit (inferred from context)
    Implicit,
}

/// History of an entity's mentions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionHistory {
    pub entity: String,
    pub first_mention: u64,
    pub last_mention: u64,
    pub mention_count: usize,
    pub context_snippets: Vec<String>,
}

impl MentionHistory {
    /// Create new mention history
    pub fn new(entity: String) -> Self {
        let now = Self::current_timestamp();
        Self {
            entity,
            first_mention: now,
            last_mention: now,
            mention_count: 1,
            context_snippets: Vec::new(),
        }
    }

    /// Record a mention
    pub fn record_mention(&mut self, _turn: usize) {
        self.last_mention = Self::current_timestamp();
        self.mention_count += 1;
    }

    /// Get current Unix timestamp
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// A single entity mention in the timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMention {
    pub turn: usize,
    pub entity: String,
    pub mention_type: MentionType,
    pub file_context: Option<PathBuf>,
}

/// A user message for context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub turn: usize,
    pub content: String,
    pub entities: Vec<String>,
}

/// Pronoun reference waiting to be resolved
#[derive(Debug, Clone)]
pub struct PronounReference {
    pub pronoun: String,
    pub turn: usize,
    pub context: String,
}

/// Conversation memory system
pub struct ConversationMemory {
    /// Mentioned entities with history
    mentioned_entities: HashMap<String, MentionHistory>,

    /// Timeline of entity mentions
    entity_timeline: VecDeque<EntityMention>,

    /// Recent user messages
    recent_user_messages: VecDeque<UserMessage>,

    /// Recent file contexts (files mentioned or edited)
    recent_file_contexts: VecDeque<PathBuf>,

    /// Unresolved pronouns
    unresolved_pronouns: Vec<PronounReference>,

    /// Resolved reference mappings
    resolved_references: HashMap<String, String>,

    /// Current turn number
    current_turn: usize,
}

impl Default for ConversationMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationMemory {
    /// Create a new conversation memory
    pub fn new() -> Self {
        Self {
            mentioned_entities: HashMap::new(),
            entity_timeline: VecDeque::with_capacity(MAX_ENTITY_MENTIONS),
            recent_user_messages: VecDeque::with_capacity(MAX_MEMORY_TURNS),
            recent_file_contexts: VecDeque::with_capacity(20),
            unresolved_pronouns: Vec::new(),
            resolved_references: HashMap::new(),
            current_turn: 0,
        }
    }

    /// Extract entities from a message
    pub fn extract_entities(&mut self, message: &str, turn: usize) {
        self.current_turn = turn;

        let entities = self.extract_nouns_and_identifiers(message);
        let mut extracted = Vec::new();

        for entity in entities {
            self.record_entity_mention(&entity, turn, MentionType::Direct);
            extracted.push(entity);
        }

        // Store user message
        self.recent_user_messages.push_back(UserMessage {
            turn,
            content: message.to_string(),
            entities: extracted,
        });

        // Keep bounded
        while self.recent_user_messages.len() > MAX_MEMORY_TURNS {
            self.recent_user_messages.pop_front();
        }
    }

    /// Extract nouns and identifiers from text
    fn extract_nouns_and_identifiers(&self, text: &str) -> Vec<String> {
        let mut entities = Vec::new();

        // Simple extraction: capitalize words, camelCase, PascalCase
        for word in text.split_whitespace() {
            let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric());

            // Skip empty and short words
            if cleaned.len() < 3 {
                continue;
            }

            // Capitalized words (likely proper nouns)
            if cleaned.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                entities.push(cleaned.to_string());
                continue;
            }

            // camelCase or PascalCase (likely identifiers)
            let has_mixed_case = cleaned
                .chars()
                .any(|c| c.is_uppercase()) && cleaned.chars().any(|c| c.is_lowercase());

            if has_mixed_case {
                entities.push(cleaned.to_string());
            }
        }

        entities
    }

    /// Record an entity mention
    fn record_entity_mention(&mut self, entity: &str, turn: usize, mention_type: MentionType) {
        let entity_lower = entity.to_lowercase();

        // Update or create history
        self.mentioned_entities
            .entry(entity_lower.clone())
            .and_modify(|history| history.record_mention(turn))
            .or_insert_with(|| MentionHistory::new(entity.to_string()));

        // Add to timeline
        self.entity_timeline.push_back(EntityMention {
            turn,
            entity: entity.to_string(),
            mention_type,
            file_context: None,
        });

        // Keep bounded
        while self.entity_timeline.len() > MAX_ENTITY_MENTIONS {
            self.entity_timeline.pop_front();
        }
    }

    /// Get recent entities (N most recent)
    pub fn get_recent_entities(&self, count: usize) -> Vec<String> {
        self.entity_timeline
            .iter()
            .rev()
            .take(count)
            .map(|mention| mention.entity.clone())
            .collect()
    }

    /// Resolve a pronoun to an entity
    pub fn resolve_pronoun(&self, pronoun: &str, turn: usize) -> Option<String> {
        let pronoun_lower = pronoun.to_lowercase();

        match pronoun_lower.as_str() {
            "it" => {
                // Look back at last mentioned entity
                self.entity_timeline
                    .iter()
                    .rev()
                    .find(|m| m.turn < turn)
                    .map(|m| m.entity.clone())
            }
            "that" | "this" => {
                // Look at last direct mention
                self.entity_timeline
                    .iter()
                    .rev()
                    .filter(|m| m.turn < turn)
                    .find(|m| matches!(m.mention_type, MentionType::Direct))
                    .map(|m| m.entity.clone())
            }
            "those" | "these" => {
                // Multiple entities - return most recent direct mention
                self.entity_timeline
                    .iter()
                    .rev()
                    .filter(|m| m.turn < turn && matches!(m.mention_type, MentionType::Direct))
                    .take(2)
                    .map(|m| m.entity.clone())
                    .next()
            }
            _ => None,
        }
    }

    /// Get all mentioned entities
    pub fn mentioned_entities(&self) -> &HashMap<String, MentionHistory> {
        &self.mentioned_entities
    }

    /// Get entity mention count
    pub fn mention_count(&self, entity: &str) -> usize {
        self.mentioned_entities
            .get(&entity.to_lowercase())
            .map(|h| h.mention_count)
            .unwrap_or(0)
    }

    /// Add file context
    pub fn add_file_context(&mut self, file: PathBuf) {
        self.recent_file_contexts.push_back(file);

        // Keep bounded
        while self.recent_file_contexts.len() > 20 {
            self.recent_file_contexts.pop_front();
        }
    }

    /// Get recent file contexts
    pub fn recent_file_contexts(&self, count: usize) -> Vec<&PathBuf> {
        self.recent_file_contexts
            .iter()
            .rev()
            .take(count)
            .collect()
    }

    /// Check if entity was recently mentioned
    pub fn was_recently_mentioned(&self, entity: &str, within_turns: usize) -> bool {
        let cutoff_turn = self.current_turn.saturating_sub(within_turns);

        self.entity_timeline
            .iter()
            .rev()
            .any(|m| {
                m.entity.eq_ignore_ascii_case(entity) && m.turn >= cutoff_turn
            })
    }

    /// Get context summary for recent conversation
    pub fn get_context_summary(&self, turns: usize) -> String {
        let messages: Vec<_> = self.recent_user_messages
            .iter()
            .rev()
            .take(turns)
            .collect();

        if messages.is_empty() {
            return String::from("No recent context available");
        }

        let mut summary = String::from("Recent conversation:\n");
        for msg in messages.iter().rev() {
            summary.push_str(&format!("Turn {}: {}\n", msg.turn, msg.content));
        }

        summary
    }

    /// Clear old data to free memory
    pub fn clear_old_data(&mut self, keep_turns: usize) {
        let cutoff_turn = self.current_turn.saturating_sub(keep_turns);

        // Remove old entity timeline entries
        self.entity_timeline.retain(|m| m.turn >= cutoff_turn);

        // Remove old user messages
        self.recent_user_messages.retain(|m| m.turn >= cutoff_turn);

        // Clear resolved references from old turns
        self.resolved_references.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_entities() {
        let mut memory = ConversationMemory::new();

        memory.extract_entities("Update the Sidebar component in App.tsx", 1);

        assert_eq!(memory.mentioned_entities.len(), 2); // Sidebar, App
        assert!(memory.mentioned_entities.contains_key("sidebar"));
        assert!(memory.mentioned_entities.contains_key("app"));
    }

    #[test]
    fn test_pronoun_resolution_it() {
        let mut memory = ConversationMemory::new();

        // Turn 1: Mention Sidebar
        memory.extract_entities("The Sidebar is too wide", 1);

        // Turn 2: Reference with "it"
        let resolved = memory.resolve_pronoun("it", 2);

        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), "Sidebar");
    }

    #[test]
    fn test_pronoun_resolution_that() {
        let mut memory = ConversationMemory::new();

        memory.extract_entities("Look at the Button component", 1);

        let resolved = memory.resolve_pronoun("that", 2);

        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap(), "Button");
    }

    #[test]
    fn test_recent_entities() {
        let mut memory = ConversationMemory::new();

        memory.extract_entities("Update Sidebar", 1);
        memory.extract_entities("Fix Button", 2);
        memory.extract_entities("Test Form", 3);

        let recent = memory.get_recent_entities(2);

        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0], "Form");
        assert_eq!(recent[1], "Button");
    }

    #[test]
    fn test_mention_count() {
        let mut memory = ConversationMemory::new();

        memory.extract_entities("Update Sidebar", 1);
        memory.extract_entities("The Sidebar is nice", 2);
        memory.extract_entities("Sidebar needs work", 3);

        assert_eq!(memory.mention_count("sidebar"), 3);
        assert_eq!(memory.mention_count("Button"), 0);
    }

    #[test]
    fn test_context_summary() {
        let mut memory = ConversationMemory::new();

        memory.extract_entities("First message", 1);
        memory.extract_entities("Second message", 2);

        let summary = memory.get_context_summary(2);

        assert!(summary.contains("First message"));
        assert!(summary.contains("Second message"));
    }
}
