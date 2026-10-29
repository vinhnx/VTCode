use super::*;

impl ModalListState {
    pub(crate) fn new(items: Vec<InlineListItem>, selected: Option<InlineListSelection>) -> Self {
        let converted: Vec<ModalListItem> = items
            .into_iter()
            .map(|item| {
                let is_divider = is_divider_title(&item);
                let search_value = item.search_value.as_ref().map(|value| value.to_ascii_lowercase());
                ModalListItem {
                    title: item.title,
                    subtitle: item.subtitle,
                    badge: item.badge,
                    indent: item.indent,
                    selection: item.selection,
                    search_value,
                    is_divider,
                    value: item.value,
                    badge_tone: item.badge_tone,
                    kind: item.kind,
                }
            })
            .collect();
        let total_selectable = converted.iter().filter(|item| item.selection.is_some()).count();
        let density_behavior = Self::density_behavior_for_items(&converted);
        let mut modal_state = Self {
            visible_indices: (0..converted.len()).collect(),
            items: converted,
            list_state: ListState::default(),
            total_selectable,
            filter_terms: Vec::new(),
            filter_query: None,
            viewport_rows: None,
            density_behavior,
        };
        modal_state.select_initial(selected);
        modal_state
    }

    fn density_behavior_for_items(items: &[ModalListItem]) -> ModalListDensityBehavior {
        if items
            .iter()
            .any(|item| matches!(item.selection, Some(InlineListSelection::ConfigAction(_))))
        {
            ModalListDensityBehavior::FixedComfortable
        } else {
            ModalListDensityBehavior::Adjustable
        }
    }

    pub(crate) fn current_selection(&self) -> Option<InlineListSelection> {
        self.list_state
            .selected()
            .and_then(|index| self.visible_indices.get(index))
            .and_then(|&item_index| self.items.get(item_index))
            .and_then(|item| item.selection.clone())
    }

    pub(super) fn get_best_matching_item(&self, query: &str) -> Option<String> {
        if query.is_empty() {
            return None;
        }

        let normalized_query = normalize_query(query);
        self.visible_indices
            .iter()
            .filter_map(|&idx| self.items.get(idx))
            .filter(|item| item.selection.is_some())
            .filter_map(|item| item.search_value.as_ref())
            .find(|search_value| exact_terms_match(&normalized_query, search_value))
            .cloned()
    }

    pub(super) fn select_previous(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let Some(mut index) = self.list_state.selected() else {
            if let Some(last) = self.last_selectable_index() {
                self.list_state.select(Some(last));
                if let Some(rows) = self.viewport_rows {
                    self.ensure_visible(rows);
                }
            }
            return;
        };

        while index > 0 {
            index -= 1;
            let item_index = match self.visible_indices.get(index) {
                Some(idx) => *idx,
                None => {
                    tracing::warn!("visible_indices index {index} out of bounds");
                    continue;
                }
            };
            if let Some(item) = self.items.get(item_index)
                && item.selection.is_some()
            {
                self.list_state.select(Some(index));
                if let Some(rows) = self.viewport_rows {
                    self.ensure_visible(rows);
                }
                return;
            }
        }

        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub(crate) fn select_next(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let mut index = self.list_state.selected().unwrap_or(usize::MAX);
        if index == usize::MAX {
            if let Some(first) = self.first_selectable_index() {
                self.list_state.select(Some(first));
            }
            return;
        }
        while index + 1 < self.visible_indices.len() {
            index += 1;
            let item_index = self.visible_indices[index];
            if self.items[item_index].selection.is_some() {
                self.list_state.select(Some(index));
                if let Some(rows) = self.viewport_rows {
                    self.ensure_visible(rows);
                }
                break;
            }
        }
    }

    pub(super) fn select_first(&mut self) {
        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub(crate) fn select_last(&mut self) {
        if let Some(last) = self.last_selectable_index() {
            self.list_state.select(Some(last));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub(crate) fn selected_is_last(&self) -> bool {
        let Some(selected) = self.list_state.selected() else {
            return false;
        };
        self.last_selectable_index().is_some_and(|last| selected == last)
    }

    pub(super) fn select_nth_selectable(&mut self, target_index: usize) -> bool {
        let mut count = 0usize;
        for (visible_pos, &item_index) in self.visible_indices.iter().enumerate() {
            if self.items[item_index].selection.is_some() {
                if count == target_index {
                    self.list_state.select(Some(visible_pos));
                    if let Some(rows) = self.viewport_rows {
                        self.ensure_visible(rows);
                    }
                    return true;
                }
                count += 1;
            }
        }
        false
    }

    /// Max numbered shortcut: digit keys 1-9 jump to the nth visible option.
    pub(super) const MAX_NUMBERED_SHORTCUT: usize = 9;

    /// Whether digit shortcuts are offered for this list: a scannable number
    /// of visible selectable options. Key routing ANDs the modal search
    /// state (digits filter while a search box is open); render callers
    /// pass their own search knowledge — one shared count gate so the two
    /// sides can never disagree.
    pub(crate) fn numbered_shortcuts(&self) -> bool {
        let count = self.visible_selectable_count();
        count > 0 && count <= Self::MAX_NUMBERED_SHORTCUT
    }

    /// 1-based shortcut number for a visible row: its position among visible
    /// selectable options, skipping dividers and other non-selectable rows.
    /// Returns `None` unless the whole list qualifies (`numbered_shortcuts`),
    /// so crowded lists never show dead numbers. Mirrors
    /// `select_nth_selectable` so badges and digit keys agree.
    pub(crate) fn shortcut_number(&self, visible_pos: usize) -> Option<usize> {
        if !self.numbered_shortcuts() {
            return None;
        }
        let &item_index = self.visible_indices.get(visible_pos)?;
        self.items.get(item_index)?.selection.as_ref()?;
        let ordinal = self
            .visible_indices
            .iter()
            .take(visible_pos + 1)
            .filter(|&&idx| self.items.get(idx).is_some_and(|item| item.selection.is_some()))
            .count();
        (ordinal <= Self::MAX_NUMBERED_SHORTCUT).then_some(ordinal)
    }

    pub(super) fn page_up(&mut self) {
        let step = self.page_step();
        if step == 0 {
            self.select_previous();
            return;
        }
        for _ in 0..step {
            let before = self.list_state.selected();
            self.select_previous();
            if self.list_state.selected() == before {
                break;
            }
        }
    }

    pub(super) fn page_down(&mut self) {
        let step = self.page_step();
        if step == 0 {
            self.select_next();
            return;
        }
        for _ in 0..step {
            let before = self.list_state.selected();
            self.select_next();
            if self.list_state.selected() == before {
                break;
            }
        }
    }

    pub(crate) fn set_viewport_rows(&mut self, rows: u16) {
        self.viewport_rows = Some(rows);
        if rows == 0 {
            *self.list_state.offset_mut() = 0;
            return;
        }
        let max_offset = self.max_scroll_offset();
        if self.list_state.offset() > max_offset {
            *self.list_state.offset_mut() = max_offset;
        }
    }

    pub(crate) fn max_scroll_offset(&self) -> usize {
        let Some(rows) = self.viewport_rows else {
            return self.visible_indices.len().saturating_sub(1);
        };
        if rows == 0 {
            return 0;
        }
        self.visible_indices.len().saturating_sub(rows as usize)
    }

    pub(crate) fn ensure_visible(&mut self, viewport: u16) {
        let Some(selected) = self.list_state.selected() else {
            return;
        };
        if viewport == 0 {
            return;
        }
        let visible = viewport as usize;
        let max_offset = self.visible_indices.len().saturating_sub(visible.max(1));
        let offset = self.list_state.offset().min(max_offset);
        *self.list_state.offset_mut() = if selected < offset {
            selected
        } else if selected >= offset + visible {
            (selected + 1 - visible).min(max_offset)
        } else {
            offset
        };
    }

    pub(crate) fn apply_search(&mut self, query: &str, fuzzy: bool) {
        let preferred = self.current_selection();
        self.apply_search_with_preference(query, preferred, fuzzy);
    }

    pub(crate) fn apply_search_with_preference(
        &mut self,
        query: &str,
        preferred: Option<InlineListSelection>,
        fuzzy: bool,
    ) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            if self.filter_query.is_none() {
                if preferred.is_some() && self.current_selection() != preferred {
                    self.select_initial(preferred);
                }
                return;
            }
            self.visible_indices = (0..self.items.len()).collect();
            self.filter_terms.clear();
            self.filter_query = None;
            self.select_initial(preferred);
            return;
        }

        if self.filter_query.as_deref() == Some(trimmed) {
            if preferred.is_some() && self.current_selection() != preferred {
                self.select_initial(preferred);
            }
            return;
        }

        let normalized_query = normalize_query(trimmed);
        let terms = normalized_query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .map(|term| term.to_owned())
            .collect::<Vec<_>>();

        if fuzzy {
            let mut fuzzy_query = FuzzyQuery::new(&normalized_query);
            self.visible_indices = self.fuzzy_visible_indices(&mut fuzzy_query);
            self.filter_terms = terms;
            self.filter_query = Some(trimmed.to_owned());
            self.select_initial(preferred);
            return;
        }

        let mut indices = Vec::new();
        let mut pending_divider: Option<usize> = None;
        let mut current_header: Option<usize> = None;
        let mut header_matches = false;
        let mut header_included = false;

        for (index, item) in self.items.iter().enumerate() {
            if item.is_divider {
                pending_divider = Some(index);
                current_header = None;
                header_matches = false;
                header_included = false;
                continue;
            }

            if item.is_header() {
                current_header = Some(index);
                header_matches = item.matches(&normalized_query);
                header_included = false;
                if header_matches {
                    if let Some(divider_index) = pending_divider.take() {
                        indices.push(divider_index);
                    }
                    indices.push(index);
                    header_included = true;
                }
                continue;
            }

            let item_matches = item.matches(&normalized_query);
            let include_item = header_matches || item_matches;
            if include_item {
                if let Some(divider_index) = pending_divider.take() {
                    indices.push(divider_index);
                }
                if let Some(header_index) = current_header
                    && !header_included
                {
                    indices.push(header_index);
                    header_included = true;
                }
                indices.push(index);
            }
        }
        self.visible_indices = indices;
        self.filter_terms = terms;
        self.filter_query = Some(trimmed.to_owned());
        self.select_initial(preferred);
    }

    /// Fuzzy filter with relevance ranking. Items are grouped into
    /// divider/header/children blocks (selectables without a preceding header
    /// rank individually); blocks are ordered by their best fuzzy score while
    /// a matching header keeps its whole group, preserving curated order
    /// within blocks.
    fn fuzzy_visible_indices(&self, query: &mut FuzzyQuery) -> Vec<usize> {
        /// One child item of a header block: list index plus its fuzzy score
        /// (`None` when the item does not match the query).
        struct ScoredIndex {
            index: usize,
            score: Option<u32>,
        }

        /// A divider/header/children run ranked as one unit; a matching header
        /// keeps all of its children, otherwise only matching children show.
        struct FuzzySearchBlock {
            divider_index: Option<usize>,
            header_index: Option<usize>,
            header_score: Option<u32>,
            child_scores: Vec<ScoredIndex>,
        }

        impl FuzzySearchBlock {
            fn best_score(&self) -> Option<u32> {
                self.header_score
                    .into_iter()
                    .chain(self.child_scores.iter().filter_map(|child| child.score))
                    .max()
            }
        }

        let mut blocks: Vec<FuzzySearchBlock> = Vec::new();
        let mut open_block: Option<FuzzySearchBlock> = None;
        let mut pending_divider: Option<usize> = None;

        for (index, item) in self.items.iter().enumerate() {
            if item.is_divider {
                if let Some(block) = open_block.take() {
                    blocks.push(block);
                }
                pending_divider = Some(index);
                continue;
            }

            let haystack = item.search_haystack();
            let score = query.score(&haystack);

            if item.is_header() {
                if let Some(block) = open_block.take() {
                    blocks.push(block);
                }
                open_block = Some(FuzzySearchBlock {
                    divider_index: pending_divider.take(),
                    header_index: Some(index),
                    header_score: score,
                    child_scores: Vec::new(),
                });
                continue;
            }

            match open_block.as_mut() {
                // Selectables directly after a header belong to its block.
                Some(block) if block.header_index.is_some() => {
                    block.child_scores.push(ScoredIndex { index, score });
                }
                _ => {
                    if let Some(block) = open_block.take() {
                        blocks.push(block);
                    }
                    open_block = Some(FuzzySearchBlock {
                        divider_index: pending_divider.take(),
                        header_index: None,
                        header_score: None,
                        child_scores: vec![ScoredIndex { index, score }],
                    });
                }
            }
        }
        if let Some(block) = open_block.take() {
            blocks.push(block);
        }

        blocks.retain(|block| block.best_score().is_some());
        blocks.sort_by_key(|block| std::cmp::Reverse(block.best_score()));

        let mut indices = Vec::new();
        for block in blocks {
            if let Some(divider_index) = block.divider_index {
                indices.push(divider_index);
            }
            if let Some(header_index) = block.header_index {
                indices.push(header_index);
            }
            let header_matched = block.header_score.is_some();
            for child in block.child_scores {
                if header_matched || child.score.is_some() {
                    indices.push(child.index);
                }
            }
        }
        indices
    }

    fn select_initial(&mut self, preferred: Option<InlineListSelection>) {
        let mut selection_index = preferred.and_then(|needle| {
            self.visible_indices
                .iter()
                .position(|&idx| self.items[idx].selection.as_ref() == Some(&needle))
        });

        if selection_index.is_none() {
            selection_index = self.first_selectable_index();
        }

        self.list_state.select(selection_index);
        *self.list_state.offset_mut() = 0;
    }

    fn first_selectable_index(&self) -> Option<usize> {
        self.visible_indices.iter().position(|&idx| self.items[idx].selection.is_some())
    }

    fn last_selectable_index(&self) -> Option<usize> {
        self.visible_indices
            .iter()
            .rposition(|&idx| self.items[idx].selection.is_some())
    }

    pub(crate) fn filter_active(&self) -> bool {
        self.filter_query.as_ref().is_some_and(|value| !value.is_empty())
    }

    #[cfg(test)]
    pub(crate) fn filter_query(&self) -> Option<&str> {
        self.filter_query.as_deref()
    }

    pub(crate) fn highlight_terms(&self) -> &[String] {
        &self.filter_terms
    }

    pub(crate) fn visible_selectable_count(&self) -> usize {
        self.visible_indices
            .iter()
            .filter(|&&idx| self.items[idx].selection.is_some())
            .count()
    }

    pub(crate) fn total_selectable(&self) -> usize {
        self.total_selectable
    }

    pub(crate) fn non_filter_summary_text(&self, footer_hint: Option<&str>) -> Option<String> {
        if !self.has_non_filter_summary(footer_hint) {
            return None;
        }
        match self.density_behavior {
            ModalListDensityBehavior::FixedComfortable => Some(config_list_navigation_hint()),
            ModalListDensityBehavior::Adjustable => footer_hint.filter(|hint| !hint.is_empty()).map(ToOwned::to_owned),
        }
    }

    /// Summary rows above the list: filter/keyboard hint, plus one extra row
    /// when a status strip is shown (`has_status`).
    pub(crate) fn summary_line_rows(&self, footer_hint: Option<&str>, has_status: bool) -> usize {
        let mut rows = 0;
        if has_status {
            rows += 1;
        }
        if self.filter_active() || self.has_non_filter_summary(footer_hint) {
            rows += 1;
        }
        rows
    }

    fn has_non_filter_summary(&self, footer_hint: Option<&str>) -> bool {
        match self.density_behavior {
            ModalListDensityBehavior::FixedComfortable => true,
            ModalListDensityBehavior::Adjustable => footer_hint.is_some_and(|hint| !hint.is_empty()),
        }
    }

    fn page_step(&self) -> usize {
        let rows = self.viewport_rows.unwrap_or(0).max(1);
        usize::from(rows)
    }
}
