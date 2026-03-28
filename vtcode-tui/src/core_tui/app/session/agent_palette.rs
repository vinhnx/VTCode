use crate::core_tui::session::list_navigator::ListNavigator;

const PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEntry {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
}

pub struct AgentPalette {
    all_agents: Vec<AgentEntry>,
    filtered_agents: Vec<AgentEntry>,
    navigator: ListNavigator,
    filter_query: String,
    filter_cache: hashbrown::HashMap<String, Vec<AgentEntry>>,
}

impl AgentPalette {
    pub fn new() -> Self {
        Self {
            all_agents: Vec::new(),
            filtered_agents: Vec::new(),
            navigator: ListNavigator::new(),
            filter_query: String::new(),
            filter_cache: hashbrown::HashMap::new(),
        }
    }

    pub fn load_agents(&mut self, agents: Vec<AgentEntry>) {
        self.all_agents = agents;
        self.all_agents
            .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        self.apply_filter();
        self.navigator.select_first();
    }

    pub fn set_filter(&mut self, query: String) {
        self.filter_query = query.clone();
        if let Some(cached) = self.filter_cache.get(&query) {
            self.filtered_agents = cached.clone();
        } else {
            self.apply_filter();
            if !query.is_empty() && self.filter_cache.len() < 50 {
                self.filter_cache
                    .insert(query, self.filtered_agents.clone());
            }
        }
        self.navigator.set_item_count(self.filtered_agents.len());
        self.navigator.select_first();
    }

    fn apply_filter(&mut self) {
        if self.filter_query.is_empty() {
            self.filtered_agents = self.all_agents.clone();
            self.navigator.set_item_count(self.filtered_agents.len());
            return;
        }

        let query = self.filter_query.to_ascii_lowercase();
        let mut matches = self
            .all_agents
            .iter()
            .filter_map(|entry| {
                let haystack = format!(
                    "{} {}",
                    entry.name,
                    entry.description.as_deref().unwrap_or_default()
                )
                .to_ascii_lowercase();
                if !haystack.contains(&query) {
                    return None;
                }

                let mut score = 0usize;
                if entry.name.eq_ignore_ascii_case(&query) {
                    score += 1000;
                } else if entry.name.to_ascii_lowercase().starts_with(&query) {
                    score += 500;
                }
                if haystack.contains(&query) {
                    score += 100;
                }
                Some((score, entry.clone()))
            })
            .collect::<Vec<_>>();

        matches.sort_unstable_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.name.to_lowercase().cmp(&right.1.name.to_lowercase()))
        });
        self.filtered_agents = matches.into_iter().map(|(_, entry)| entry).collect();
        self.navigator.set_item_count(self.filtered_agents.len());
    }

    pub fn move_selection_up(&mut self) {
        self.navigator.move_up();
    }

    pub fn move_selection_down(&mut self) {
        self.navigator.move_down();
    }

    pub fn select_best_match(&mut self) {
        self.navigator.select_first();
    }

    pub fn get_selected(&self) -> Option<&AgentEntry> {
        self.navigator
            .selected()
            .and_then(|index| self.filtered_agents.get(index))
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        self.navigator.select_index(index)
    }

    pub fn current_page_items(&self) -> Vec<(usize, &AgentEntry, bool)> {
        let start = self.current_page_index() * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.filtered_agents.len());
        let selected = self.navigator.selected();

        self.filtered_agents[start..end]
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let global_idx = start + idx;
                (global_idx, entry, selected == Some(global_idx))
            })
            .collect()
    }

    pub fn has_more_items(&self) -> bool {
        if self.filtered_agents.is_empty() {
            return false;
        }
        let end = ((self.current_page_index() + 1) * PAGE_SIZE).min(self.filtered_agents.len());
        end < self.filtered_agents.len()
    }

    pub fn current_page_number(&self) -> usize {
        if self.filtered_agents.is_empty() {
            1
        } else {
            self.current_page_index() + 1
        }
    }

    pub fn total_items(&self) -> usize {
        self.filtered_agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filtered_agents.is_empty()
    }

    pub fn has_agents(&self) -> bool {
        !self.all_agents.is_empty()
    }

    pub fn filter_query(&self) -> &str {
        &self.filter_query
    }

    fn current_page_index(&self) -> usize {
        self.navigator.selected().unwrap_or(0) / PAGE_SIZE
    }
}

pub fn extract_agent_reference(input: &str, cursor: usize) -> Option<(usize, usize, String)> {
    if cursor > input.len() {
        return None;
    }

    let bytes = input.as_bytes();
    let mut token_start = cursor;
    while token_start > 0 && !bytes[token_start - 1].is_ascii_whitespace() {
        token_start -= 1;
    }

    let mut token_end = cursor;
    while token_end < bytes.len() && !bytes[token_end].is_ascii_whitespace() {
        token_end += 1;
    }

    let token = &input[token_start..token_end];
    if !token.starts_with("@agent-") {
        return None;
    }

    let prefix_starts_token = token_start == 0
        || input[..token_start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
    if !prefix_starts_token {
        return None;
    }

    Some((token_start, token_end, token["@agent-".len()..].to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{AgentEntry, AgentPalette, extract_agent_reference};

    #[test]
    fn extracts_local_agent_reference() {
        let input = "use @agent-explorer now";
        let cursor = input.find(" now").unwrap_or(input.len());
        assert_eq!(
            extract_agent_reference(input, cursor),
            Some((4, cursor, "explorer".to_string()))
        );
    }

    #[test]
    fn extracts_plugin_agent_reference() {
        let input = "use @agent-github:reviewer now";
        let cursor = input.find(" now").unwrap_or(input.len());
        assert_eq!(
            extract_agent_reference(input, cursor),
            Some((4, cursor, "github:reviewer".to_string()))
        );
    }

    #[test]
    fn ignores_non_agent_reference() {
        let input = "use @src/main.rs now";
        assert_eq!(extract_agent_reference(input, 16), None);
    }

    #[test]
    fn ranks_prefix_matches_first() {
        let mut palette = AgentPalette::new();
        palette.load_agents(vec![
            AgentEntry {
                name: "worker".to_string(),
                display_name: "@agent-worker".to_string(),
                description: Some("Write-capable worker".to_string()),
            },
            AgentEntry {
                name: "explorer".to_string(),
                display_name: "@agent-explorer".to_string(),
                description: Some("Read-only explorer".to_string()),
            },
        ]);

        palette.set_filter("exp".to_string());
        assert_eq!(
            palette.get_selected().map(|entry| entry.name.as_str()),
            Some("explorer")
        );
    }
}
