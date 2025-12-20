//! Entity resolution for vibe coding support
//!
//! This module provides fuzzy entity matching to resolve vague terms like
//! "the sidebar" or "that button" to actual workspace entities (files, components, etc.)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum number of entity matches to return
const MAX_ENTITY_MATCHES: usize = 5;

/// Maximum number of recent edits to track
const MAX_RECENT_EDITS: usize = 50;

/// Location of an entity within a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLocation {
    pub path: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub content_preview: String,
}

/// A matched entity with confidence score
#[derive(Debug, Clone)]
pub struct EntityMatch {
    pub entity: String,
    pub locations: Vec<FileLocation>,
    pub confidence: f32,
    pub recency_score: f32,
    pub mention_score: f32,
    pub proximity_score: f32,
}

impl EntityMatch {
    /// Calculate total score for ranking
    pub fn total_score(&self) -> f32 {
        self.confidence + self.recency_score + self.mention_score + self.proximity_score
    }
}

/// Reference to an entity that was recently edited
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityReference {
    pub entity: String,
    pub file: PathBuf,
    pub timestamp: u64,
}

/// Value found in a style file (CSS, SCSS, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleValue {
    pub property: String,
    pub value: String,
    pub file: PathBuf,
    pub line: usize,
}

/// Index of entities in the workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityIndex {
    /// UI components (Sidebar, Button, etc.)
    pub ui_components: HashMap<String, Vec<FileLocation>>,

    /// Functions and methods
    pub functions: HashMap<String, Vec<FileLocation>>,

    /// Classes and structs
    pub classes: HashMap<String, Vec<FileLocation>>,

    /// Style properties (padding, color, etc.)
    pub style_properties: HashMap<String, Vec<StyleValue>>,

    /// Config keys
    pub config_keys: HashMap<String, Vec<FileLocation>>,

    /// Recent edits for recency ranking
    pub recent_edits: VecDeque<EntityReference>,

    /// Last mentioned entities with timestamps
    pub last_mentioned: HashMap<String, u64>,

    /// Last update timestamp
    pub last_updated: u64,
}

impl Default for EntityIndex {
    fn default() -> Self {
        Self {
            ui_components: HashMap::new(),
            functions: HashMap::new(),
            classes: HashMap::new(),
            style_properties: HashMap::new(),
            config_keys: HashMap::new(),
            recent_edits: VecDeque::with_capacity(MAX_RECENT_EDITS),
            last_mentioned: HashMap::new(),
            last_updated: Self::current_timestamp(),
        }
    }
}

impl EntityIndex {
    /// Get current Unix timestamp
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Record an entity mention for recency tracking
    pub fn record_mention(&mut self, entity: &str) {
        self.last_mentioned.insert(
            entity.to_lowercase(),
            Self::current_timestamp(),
        );
    }

    /// Record a recent edit
    pub fn record_edit(&mut self, entity: String, file: PathBuf) {
        let reference = EntityReference {
            entity,
            file,
            timestamp: Self::current_timestamp(),
        };

        self.recent_edits.push_back(reference);

        // Keep bounded
        while self.recent_edits.len() > MAX_RECENT_EDITS {
            self.recent_edits.pop_front();
        }
    }

    /// Check if file was recently edited
    pub fn was_recently_edited(&self, file: &Path, within_seconds: u64) -> bool {
        let cutoff = Self::current_timestamp().saturating_sub(within_seconds);
        self.recent_edits
            .iter()
            .any(|r| r.file == file && r.timestamp >= cutoff)
    }
}

/// Entity resolver for fuzzy matching
pub struct EntityResolver {
    /// The entity index
    index: EntityIndex,

    /// Workspace root for path resolution
    workspace_root: PathBuf,

    /// Cache file path
    cache_path: Option<PathBuf>,
}

impl EntityResolver {
    /// Create a new entity resolver
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            index: EntityIndex::default(),
            workspace_root,
            cache_path: None,
        }
    }

    /// Create with cache file path
    pub fn with_cache(workspace_root: PathBuf, cache_path: PathBuf) -> Self {
        Self {
            index: EntityIndex::default(),
            workspace_root,
            cache_path: Some(cache_path),
        }
    }

    /// Load index from cache file
    pub async fn load_cache(&mut self) -> Result<()> {
        if let Some(cache_path) = &self.cache_path {
            if cache_path.exists() {
                let content = tokio::fs::read_to_string(cache_path)
                    .await
                    .with_context(|| format!("Failed to read entity cache at {:?}", cache_path))?;

                self.index = serde_json::from_str(&content)
                    .with_context(|| "Failed to deserialize entity cache")?;
            }
        }
        Ok(())
    }

    /// Save index to cache file
    pub async fn save_cache(&self) -> Result<()> {
        if let Some(cache_path) = &self.cache_path {
            let content = serde_json::to_string_pretty(&self.index)
                .with_context(|| "Failed to serialize entity cache")?;

            tokio::fs::write(cache_path, content)
                .await
                .with_context(|| format!("Failed to write entity cache to {:?}", cache_path))?;
        }
        Ok(())
    }

    /// Check if the entity index is empty
    pub fn index_is_empty(&self) -> bool {
        self.index.ui_components.is_empty()
            && self.index.functions.is_empty()
            && self.index.classes.is_empty()
    }

    /// Resolve a vague term to entity matches
    pub fn resolve(&self, term: &str) -> Option<EntityMatch> {
        let matches = self.find_entity_fuzzy(term);

        // Return best match if any
        matches.into_iter().max_by(|a, b| {
            a.total_score().partial_cmp(&b.total_score()).unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Find entities using fuzzy matching
    fn find_entity_fuzzy(&self, term: &str) -> Vec<EntityMatch> {
        let term_lower = term.to_lowercase();
        let mut matches = Vec::new();

        // Search UI components
        self.search_hashmap(&self.index.ui_components, &term_lower, &mut matches);

        // Search functions
        self.search_hashmap(&self.index.functions, &term_lower, &mut matches);

        // Search classes
        self.search_hashmap(&self.index.classes, &term_lower, &mut matches);

        // Sort by total score and limit
        matches.sort_by(|a, b| {
            b.total_score().partial_cmp(&a.total_score()).unwrap_or(std::cmp::Ordering::Equal)
        });
        matches.truncate(MAX_ENTITY_MATCHES);

        matches
    }

    /// Search a hashmap for matching entities
    fn search_hashmap(
        &self,
        map: &HashMap<String, Vec<FileLocation>>,
        term: &str,
        matches: &mut Vec<EntityMatch>,
    ) {
        for (entity, locations) in map {
            let entity_lower = entity.to_lowercase();

            // Exact match
            if entity_lower == term {
                matches.push(EntityMatch {
                    entity: entity.clone(),
                    locations: locations.clone(),
                    confidence: 1.0,
                    recency_score: self.calculate_recency_score(&entity_lower),
                    mention_score: self.calculate_mention_score(&entity_lower),
                    proximity_score: 0.0,
                });
                continue;
            }

            // Case-insensitive substring match
            if entity_lower.contains(term) {
                let confidence = term.len() as f32 / entity_lower.len() as f32;
                matches.push(EntityMatch {
                    entity: entity.clone(),
                    locations: locations.clone(),
                    confidence: confidence * 0.8, // Slightly lower than exact
                    recency_score: self.calculate_recency_score(&entity_lower),
                    mention_score: self.calculate_mention_score(&entity_lower),
                    proximity_score: 0.0,
                });
                continue;
            }

            // Fuzzy match using Levenshtein distance
            let distance = levenshtein_distance(term, &entity_lower);
            if distance <= 2 {
                let confidence = 1.0 - (distance as f32 / term.len().max(entity_lower.len()) as f32);
                matches.push(EntityMatch {
                    entity: entity.clone(),
                    locations: locations.clone(),
                    confidence: confidence * 0.6, // Lower confidence for fuzzy
                    recency_score: self.calculate_recency_score(&entity_lower),
                    mention_score: self.calculate_mention_score(&entity_lower),
                    proximity_score: 0.0,
                });
            }
        }
    }

    /// Calculate recency score based on recent edits
    fn calculate_recency_score(&self, entity: &str) -> f32 {
        let now = EntityIndex::current_timestamp();

        // Check if entity was recently edited (within 5 minutes)
        if let Some(edit) = self.index.recent_edits.iter().rev().find(|e| e.entity.to_lowercase() == entity) {
            let age_seconds = now.saturating_sub(edit.timestamp);
            if age_seconds < 300 {
                // Score decays over 5 minutes
                return 0.3 * (1.0 - (age_seconds as f32 / 300.0));
            }
        }

        0.0
    }

    /// Calculate mention score based on conversation history
    fn calculate_mention_score(&self, entity: &str) -> f32 {
        if let Some(&timestamp) = self.index.last_mentioned.get(entity) {
            let now = EntityIndex::current_timestamp();
            let age_seconds = now.saturating_sub(timestamp);

            // Score decays over 10 minutes
            if age_seconds < 600 {
                return 0.2 * (1.0 - (age_seconds as f32 / 600.0));
            }
        }

        0.0
    }

    /// Record a mention for recency tracking
    pub fn record_mention(&mut self, entity: &str) {
        self.index.record_mention(entity);
    }

    /// Record an edit for recency tracking
    pub fn record_edit(&mut self, entity: String, file: PathBuf) {
        self.index.record_edit(entity, file);
    }

    /// Get mutable access to the index for building
    pub fn index_mut(&mut self) -> &mut EntityIndex {
        &mut self.index
    }

    /// Get read-only access to the index
    pub fn index(&self) -> &EntityIndex {
        &self.index
    }
}

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    for (i, a_char) in a_chars.iter().enumerate() {
        for (j, b_char) in b_chars.iter().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };

            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[a_len][b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("", ""), 0);
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
        assert_eq!(levenshtein_distance("hello", "hallo"), 1);
        assert_eq!(levenshtein_distance("sidebar", "sidbar"), 1);
        assert_eq!(levenshtein_distance("button", "btn"), 3);
    }

    #[test]
    fn test_entity_match_scoring() {
        let match1 = EntityMatch {
            entity: "Sidebar".to_string(),
            locations: vec![],
            confidence: 1.0,
            recency_score: 0.3,
            mention_score: 0.2,
            proximity_score: 0.0,
        };

        assert_eq!(match1.total_score(), 1.5);
    }

    #[tokio::test]
    async fn test_entity_resolver_exact_match() {
        let mut resolver = EntityResolver::new(PathBuf::from("/test"));

        resolver.index_mut().ui_components.insert(
            "Sidebar".to_string(),
            vec![FileLocation {
                path: PathBuf::from("src/Sidebar.tsx"),
                line_start: 1,
                line_end: 50,
                content_preview: "export const Sidebar = () => {}".to_string(),
            }],
        );

        let result = resolver.resolve("sidebar");
        assert!(result.is_some());

        let matched = result.unwrap();
        assert_eq!(matched.entity, "Sidebar");
        assert_eq!(matched.confidence, 1.0);
    }
}
