#[derive(Clone, Debug, Default)]
pub(crate) struct ListNavigator {
    selected: Option<usize>,
    scroll_offset: usize,
    visible_rows: usize,
    item_count: usize,
}

impl ListNavigator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_item_count(&mut self, count: usize) {
        self.item_count = count;
        if count == 0 {
            self.selected = None;
            self.scroll_offset = 0;
            return;
        }

        if let Some(selected) = self.selected
            && selected >= count
        {
            self.selected = Some(count - 1);
        }
        self.ensure_selection_visible();
    }

    pub(crate) fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows;
        self.ensure_selection_visible();
    }

    pub(crate) fn visible_rows(&self) -> usize {
        self.visible_rows
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(crate) fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    pub(crate) fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected.filter(|index| *index < self.item_count);
        self.ensure_selection_visible();
    }

    pub(crate) fn select_index(&mut self, index: usize) -> bool {
        if index >= self.item_count {
            return false;
        }

        if self.selected == Some(index) {
            return false;
        }

        self.selected = Some(index);
        self.ensure_selection_visible();
        true
    }

    pub(crate) fn select_first(&mut self) -> bool {
        if self.item_count == 0 {
            return false;
        }

        self.select_index(0)
    }

    pub(crate) fn select_last(&mut self) -> bool {
        if self.item_count == 0 {
            return false;
        }

        self.select_index(self.item_count - 1)
    }

    pub(crate) fn move_up(&mut self) -> bool {
        if self.item_count == 0 {
            return false;
        }

        let current = self.selected.unwrap_or(0);
        let next = if current > 0 {
            current - 1
        } else {
            self.item_count - 1
        };
        self.select_index(next)
    }

    pub(crate) fn move_down(&mut self) -> bool {
        if self.item_count == 0 {
            return false;
        }

        let current = self.selected.unwrap_or(self.item_count - 1);
        let next = if current + 1 < self.item_count {
            current + 1
        } else {
            0
        };
        self.select_index(next)
    }

    pub(crate) fn page_up(&mut self, step: usize) -> bool {
        if self.item_count == 0 {
            return false;
        }

        let step = step.max(1);
        let current = self.selected.unwrap_or(0);
        let next = current.saturating_sub(step);
        self.select_index(next)
    }

    pub(crate) fn page_down(&mut self, step: usize) -> bool {
        if self.item_count == 0 {
            return false;
        }

        let step = step.max(1);
        let current = self.selected.unwrap_or(0);
        let mut next = current.saturating_add(step);
        if next >= self.item_count {
            next = self.item_count - 1;
        }
        self.select_index(next)
    }

    fn ensure_selection_visible(&mut self) {
        let Some(selected) = self.selected else {
            self.scroll_offset = 0;
            return;
        };
        if self.visible_rows == 0 {
            self.scroll_offset = 0;
            return;
        }

        if selected < self.scroll_offset {
            self.scroll_offset = selected;
        } else if selected >= self.scroll_offset + self.visible_rows {
            self.scroll_offset = selected + 1 - self.visible_rows;
        }
    }
}
