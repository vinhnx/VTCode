//! Desktop-local labels for the recent workspaces ("projects") shown in the
//! sidebar and composer pickers, plus the client-local active filter.
//!
//! Tags are derived data: there is no registry. A tag exists while at least
//! one workspace carries it. Both the tag map and the active filter live in
//! `desktop/preferences.json`, so no protocol, service, or migration change is
//! needed. All functions here are pure; callers own persistence and rendering.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Maximum number of characters in a tag name.
pub(super) const MAX_TAG_CHARS: usize = 32;
/// Maximum number of tags a single workspace may carry.
pub(super) const MAX_TAGS_PER_WORKSPACE: usize = 20;

/// Normalize user input into a canonical tag name.
///
/// A leading `#` is accepted but never stored (the glyph already has other
/// meanings elsewhere in the product). Names are lowercase, single-spaced,
/// at most [`MAX_TAG_CHARS`] characters, and contain no commas or control
/// characters. `None` means the input is not a valid tag.
pub(super) fn normalize_tag(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let unhashed = trimmed.strip_prefix('#').unwrap_or(trimmed).trim();
    // Check the raw name before `split_whitespace` folds a newline or tab
    // into an ordinary space and hides the control character. Length is
    // checked on the normalized name, so repeated interior spaces do not
    // push an otherwise valid tag over the limit.
    if unhashed.is_empty() || unhashed.contains(',') || unhashed.chars().any(char::is_control) {
        return None;
    }
    let normalized = unhashed.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    (normalized.chars().count() <= MAX_TAG_CHARS).then_some(normalized)
}

/// Persisted tag state: workspace path string -> tags, plus the active filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct ProjectTagsState {
    pub(super) workspaces: BTreeMap<String, Vec<String>>,
    /// Active filter with OR semantics. Empty means no filter is applied.
    pub(super) active: Vec<String>,
}

impl ProjectTagsState {
    /// Every tag currently in use, mapped to the number of workspaces carrying
    /// it. Tags are deduplicated per workspace before counting.
    pub(super) fn all_tags(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for tags in self.workspaces.values() {
            for tag in tags.iter().collect::<BTreeSet<_>>() {
                *counts.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// Active tags that still exist. Stale entries are ignored, so a tag that
    /// was renamed or deleted elsewhere never filters everything out.
    pub(super) fn effective_active(&self) -> Vec<&str> {
        let existing = self.all_tags();
        let mut seen = BTreeSet::new();
        self.active
            .iter()
            .filter(|tag| existing.contains_key(*tag))
            .filter(|tag| seen.insert(tag.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// Whether a workspace belongs to the active context.
    pub(super) fn matches(&self, workspace: &str) -> bool {
        let active = self.effective_active();
        self.matches_active(workspace, &active)
    }

    /// Match against a precomputed active set so callers filtering many
    /// workspaces or sessions derive it once instead of once per item.
    pub(super) fn matches_active(&self, workspace: &str, active: &[&str]) -> bool {
        if active.is_empty() {
            return true;
        }
        self.workspaces
            .get(workspace)
            .is_some_and(|tags| active.iter().any(|tag| tags.iter().any(|existing| existing.as_str() == *tag)))
    }

    /// Recent workspace paths restricted to the active context, preserving
    /// the caller's order (most-recent first).
    pub(super) fn visible_workspaces(&self, workspaces: &[String]) -> Vec<String> {
        let active = self.effective_active();
        workspaces
            .iter()
            .filter(|path| self.matches_active(path, &active))
            .cloned()
            .collect()
    }

    /// Drop tags for workspaces that are no longer in the recent list. The
    /// map is derived from that list, so keeping evicted entries would leave
    /// phantom tags in the picker and filters that match nothing.
    pub(super) fn retain_recent(&mut self, recent: &[String]) -> bool {
        let before = self.workspaces.len();
        self.workspaces
            .retain(|path, _| recent.iter().any(|candidate| candidate == path));
        let changed = self.workspaces.len() != before;
        if changed {
            let _ = self.prune_active();
        }
        changed
    }

    /// Short label for the filter trigger: empty, a single tag, or the first
    /// tag plus the number of additional active tags (`work +1`).
    pub(super) fn active_label(&self) -> String {
        let active = self.effective_active();
        match active.as_slice() {
            [] => String::new(),
            [only] => (*only).to_owned(),
            [first, rest @ ..] => format!("{first} +{}", rest.len()),
        }
    }

    /// Replace all tags on a workspace. Returns whether anything changed.
    pub(super) fn set_workspace_tags(&mut self, workspace: &str, tags: Vec<String>) -> bool {
        // All-or-nothing: an invalid entry must never silently clear a
        // project's existing tags.
        if !tags.iter().all(|tag| normalize_tag(tag).is_some()) {
            return false;
        }
        let normalized = dedupe_tags(tags);
        if self.workspaces.get(workspace) == Some(&normalized) {
            return false;
        }
        if normalized.is_empty() {
            let _ = self.workspaces.remove(workspace);
        } else {
            let _ = self.workspaces.insert(workspace.to_owned(), normalized);
        }
        true
    }

    /// Add one tag to a workspace, accepting a leading `#`. Returns whether
    /// the workspace changed.
    pub(super) fn add_tag(&mut self, workspace: &str, raw: &str) -> bool {
        let Some(tag) = normalize_tag(raw) else {
            return false;
        };
        let tags = self.workspaces.entry(workspace.to_owned()).or_default();
        if tags.len() >= MAX_TAGS_PER_WORKSPACE || tags.contains(&tag) {
            return false;
        }
        tags.push(tag);
        true
    }

    /// Remove one tag from a workspace. Returns whether anything changed.
    pub(super) fn remove_tag(&mut self, workspace: &str, tag: &str) -> bool {
        let Some(tags) = self.workspaces.get_mut(workspace) else {
            return false;
        };
        let before = tags.len();
        tags.retain(|existing| existing != tag);
        let changed = tags.len() != before;
        if tags.is_empty() {
            let _ = self.workspaces.remove(workspace);
        }
        changed
    }

    /// Rename a tag everywhere, merging into the target when it already
    /// exists. Returns whether anything changed.
    pub(super) fn rename_tag(&mut self, from: &str, raw_to: &str) -> bool {
        let Some(to) = normalize_tag(raw_to) else {
            return false;
        };
        if from == to {
            return false;
        }
        let mut changed = false;
        let paths = self.workspaces.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            let Some(tags) = self.workspaces.get_mut(&path) else {
                continue;
            };
            if !tags.iter().any(|tag| tag == from) {
                continue;
            }
            tags.retain(|tag| tag != from);
            if !tags.contains(&to) && tags.len() < MAX_TAGS_PER_WORKSPACE {
                tags.push(to.clone());
            }
            changed = true;
        }
        for tag in &mut self.active {
            if *tag == from {
                *tag = to.clone();
                changed = true;
            }
        }
        if changed {
            self.active = dedupe_tags(self.active.clone());
            let _ = self.prune_active();
        }
        changed
    }

    /// Delete a tag from every workspace and from the active filter.
    pub(super) fn delete_tag(&mut self, tag: &str) -> bool {
        let mut changed = false;
        let paths = self.workspaces.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            changed |= self.remove_tag(&path, tag);
        }
        let before = self.active.len();
        self.active.retain(|active| active != tag);
        changed |= self.active.len() != before;
        changed
    }

    /// Drop active tags that no longer exist anywhere. Returns whether the
    /// filter changed.
    pub(super) fn prune_active(&mut self) -> bool {
        let deduped = dedupe_tags(self.active.clone());
        let existing = self.all_tags();
        let retained = deduped.into_iter().filter(|tag| existing.contains_key(tag)).collect::<Vec<_>>();
        if retained == self.active {
            return false;
        }
        self.active = retained;
        true
    }

    /// Apply the one active tag to a workspace that was just added, when
    /// exactly one tag is active. Returns whether anything changed.
    pub(super) fn tag_new_workspace(&mut self, workspace: &str) -> bool {
        // Use the user's explicit selection, not the effective one: a stale
        // tag silently ignored by the filter must not reduce a two-tag
        // context into an auto-tag decision.
        let only = match dedupe_tags(self.active.clone()).as_slice() {
            [only] => only.clone(),
            _ => return false,
        };
        if !self.all_tags().contains_key(&only) {
            return false;
        }
        self.add_tag(workspace, &only)
    }
}

/// Keep only valid, unique tags, in first-seen order.
fn dedupe_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tags.into_iter()
        .filter_map(|tag| normalize_tag(&tag))
        .filter(|tag| seen.insert(tag.clone()))
        .take(MAX_TAGS_PER_WORKSPACE)
        .collect()
}

#[cfg(test)]
mod tests;
