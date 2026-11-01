use super::*;

impl DesktopView {
    /// This thread's submitted prompts, newest first, deduplicated. Shared by
    /// the history picker and inline Up/Down/Ctrl+P/N recall.
    pub(super) fn prompt_history_entries(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        self.transcript
            .items_newest_first()
            .filter(|item| item.role == "user")
            .filter_map(|item| {
                let prompt = item.text.to_string();
                if prompt.trim().is_empty() || !seen.insert(prompt.clone()) {
                    return None;
                }
                Some(prompt)
            })
            .collect()
    }

    /// Traverse prompt history (CLI Up/Down/Ctrl+P/N parity). `older` moves
    /// toward older prompts; arrow keys only recall on single-visual-row
    /// drafts and return false otherwise so the caret keeps its native move.
    pub(super) fn recall_history(
        &mut self,
        older: bool,
        arrows: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let entries = self.prompt_history_entries();
        if entries.is_empty() {
            return false;
        }
        if arrows && !self.composer_is_single_visual_row(cx) {
            return false;
        }
        let draft = self.composer.read(cx).value();
        // A session switch can shrink the history under a live cursor.
        if self.composer_history_index.is_some_and(|ix| ix >= entries.len()) {
            self.composer_history_index = None;
        }
        let entry = match self.composer_history_index {
            None if older => {
                self.composer_history_stash = draft;
                self.composer_history_index = Some(0);
                entries.first().cloned()
            }
            None => return false,
            Some(ix) if older => {
                let next = (ix + 1).min(entries.len() - 1);
                self.composer_history_index = Some(next);
                entries.get(next).cloned()
            }
            Some(0) => {
                self.composer_history_index = None;
                let stash = std::mem::take(&mut self.composer_history_stash);
                self.composer.update(cx, |input, cx| input.replace_all(stash, window, cx));
                self.move_composer_caret_to_end(window, cx);
                // Silent writes emit no `Change`: refresh suggestions so a
                // recalled `@…` trigger still offers Tab completion.
                self.refresh_composer_suggestions_after_input_navigation(window, cx);
                cx.notify();
                return true;
            }
            Some(ix) => {
                self.composer_history_index = Some(ix - 1);
                entries.get(ix - 1).cloned()
            }
        };
        let Some(entry) = entry else {
            self.composer_history_index = None;
            return false;
        };
        self.composer.update(cx, |input, cx| input.replace_all(entry, window, cx));
        self.move_composer_caret_to_end(window, cx);
        // Silent writes emit no `Change`: refresh suggestions so a
        // recalled `@…` trigger still offers Tab completion.
        self.refresh_composer_suggestions_after_input_navigation(window, cx);
        cx.notify();
        true
    }

    pub(super) fn move_composer_caret_to_end(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.read(cx).focus_handle(cx).dispatch_action(&MoveToEnd, window, cx);
    }

    /// Wholesale programmatic composer write that preserves Kit undo history
    /// (`set_value` clears it) and parks the caret at the end. Callers
    /// replacing the draft must also reset history recall.
    pub(super) fn replace_composer_text(
        &mut self,
        text: impl Into<gpui_kit::SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer.update(cx, |input, cx| input.replace_all(text, window, cx));
        self.move_composer_caret_to_end(window, cx);
        cx.notify();
    }

    pub(super) fn reset_history_recall(&mut self) {
        self.composer_history_index = None;
        self.composer_history_stash = gpui_kit::SharedString::from("");
    }

    /// Arrow recall fires only when the composer spans a single *visual* row
    /// (CLI `is_multi_row_composer` parity): multi-row drafts — real
    /// newlines or soft wraps — consume arrows as caret moves and never
    /// traverse history. Without layout the newline check preserves the
    /// previous behavior exactly.
    fn composer_is_single_visual_row(&self, cx: &App) -> bool {
        let input = self.composer.read(cx);
        if input.display_map().display_row_count() > 1 {
            return false;
        }
        !input.value().contains('\n')
    }
}
