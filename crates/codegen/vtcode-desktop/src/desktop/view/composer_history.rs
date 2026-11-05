use super::*;

/// History-palette rows across every section (CLI `HistoryPickerState` parity:
/// merged, deduplicated, truncated to 200).
const MAX_PROMPT_HISTORY_ROWS: usize = 200;

impl DesktopView {
    pub(super) fn open_prompt_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::PromptHistory, window, cx) {
            return;
        }
        self.show_palette(PaletteKind::PromptHistory, self.history_palette_groups(), window, cx);
        // CLI parity: seed the filter from the draft. Cancel-restore is
        // automatic — the palette query is separate from the composer draft.
        let seed = self.composer.read(cx).value().trim().to_string();
        if !seed.is_empty() {
            if let Some(palette) = &self.palette {
                palette.update(cx, |state, cx| state.set_query(seed, window, cx));
            }
        }
        self.request_recent_prompts();
    }

    /// Palette groups for Ctrl+R: the live thread first, then cached
    /// cross-thread recents for the current scope. Merged newest-first,
    /// deduplicated by content with the live thread winning, capped at
    /// [`MAX_PROMPT_HISTORY_ROWS`] like the CLI picker. Session scope shows
    /// the live thread only; other scopes add recents so brand-new sessions
    /// offer history search instead of an empty palette.
    fn history_palette_groups(&self) -> Vec<(String, Vec<PaletteCommand>)> {
        let mut seen = HashSet::new();
        let mut thread = Vec::new();
        for prompt in self.prompt_history_entries() {
            if thread.len() == MAX_PROMPT_HISTORY_ROWS {
                break;
            }
            if seen.insert(prompt.clone()) {
                thread.push(prompt);
            }
        }
        let mut others = Vec::new();
        if self.history_scope != HistoryScope::Session && self.recent_prompts_scope == Some(self.history_scope) {
            for recent in &self.recent_prompts {
                if thread.len() + others.len() == MAX_PROMPT_HISTORY_ROWS {
                    break;
                }
                if recent.content.trim().is_empty() {
                    continue;
                }
                if seen.insert(recent.content.clone()) {
                    others.push(recent.content.clone());
                }
            }
        }
        let mut groups = Vec::with_capacity(2);
        if !thread.is_empty() || others.is_empty() {
            groups.push((rust_i18n::t!("app.palette.sections.this_thread").to_string(), history_commands(thread)));
        }
        if !others.is_empty() {
            groups.push((rust_i18n::t!("app.palette.sections.other_threads").to_string(), history_commands(others)));
        }
        groups
    }

    /// Switch the history scope from the palette switcher or Ctrl+S: rebuild
    /// immediately from the live thread plus any matching cache, then refresh
    /// cross-thread scopes from the service (Session needs no fetch).
    pub(super) fn set_history_scope(&mut self, scope: HistoryScope, cx: &mut Context<Self>) {
        if self.history_scope == scope {
            return;
        }
        self.history_scope = scope;
        self.refresh_history_palette();
        self.request_recent_prompts_for_scope(scope);
        cx.notify();
    }

    /// Cycle the scope (CLI Ctrl+S parity). Opens the palette when closed.
    pub(super) fn cycle_history_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_none() || self.palette_kind != PaletteKind::PromptHistory {
            self.open_prompt_history(window, cx);
            return;
        }
        self.set_history_scope(self.history_scope.cycle_next(), cx);
    }

    fn refresh_history_palette(&mut self) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::PromptHistory {
            // Kit preserves the query and selection-by-path across model
            // reinstalls; the recents section only appends rows.
            self.palette_groups = share_palette_commands(self.history_palette_groups());
        }
    }

    /// Ask the service for cross-thread recent prompts. Fire-and-forget: the
    /// palette already shows the live thread plus the cache, and the response
    /// merges underneath via [`DesktopView::apply_recent_prompts`]. The open
    /// path skips while a fetch is in flight; scope switches always refresh
    /// (responses echo their scope, so out-of-order arrivals cannot mislabel).
    fn request_recent_prompts(&mut self) {
        if self.history_scope == HistoryScope::Session || self.recent_prompts_loading {
            return;
        }
        self.request_recent_prompts_for_scope(self.history_scope);
    }

    fn request_recent_prompts_for_scope(&mut self, scope: HistoryScope) {
        if scope == HistoryScope::Session {
            return;
        }
        let Ok(params) = serde_json::to_value(protocol::RecentPromptsParams { scope }) else {
            return;
        };
        if self.request(protocol::method::SESSION_RECENT_PROMPTS, params) {
            self.recent_prompts_loading = true;
        }
    }

    /// Merge a `session/recent-prompts` response. The echoed scope tags the
    /// cache; only a matching open history palette rebuilds.
    pub(super) fn apply_recent_prompts(&mut self, result: Value) {
        self.recent_prompts_loading = false;
        let Ok(result) = serde_json::from_value::<protocol::RecentPromptsResult>(result) else {
            return;
        };
        self.recent_prompts = result
            .prompts
            .into_iter()
            .filter(|prompt| !prompt.content.trim().is_empty())
            .take(protocol::MAX_RECENT_PROMPTS)
            .collect();
        self.recent_prompts_scope = Some(result.scope);
        if result.scope == self.history_scope {
            self.refresh_history_palette();
        }
    }

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
        // Genuine input abandons recall: when the draft no longer matches the
        // entry this cursor wrote, restart traversal instead of clobbering
        // typing (or resurrecting a cleared stash). Synchronous by design —
        // `Change` effects flush after the update, too late to gate the next
        // keypress, and recall's own undo-preserving writes emit `Change`.
        if self
            .composer_history_index
            .is_some_and(|ix| entries.get(ix).map(String::as_str) != Some(draft.as_ref()))
        {
            self.reset_history_recall();
        }
        let entry = match self.composer_history_index {
            None if older => {
                self.composer_history_stash = draft;
                self.composer_history_index = Some(0);
                entries.first().cloned()
            }
            None => return false,
            Some(ix) if older => {
                if ix + 1 >= entries.len() {
                    // At the oldest (CLI `go_to_previous_history` returns None):
                    // consume the key without rewriting identical text or
                    // pushing a phantom undo step.
                    return true;
                }
                self.composer_history_index = Some(ix + 1);
                entries.get(ix + 1).cloned()
            }
            Some(0) => {
                self.composer_history_index = None;
                let stash = std::mem::take(&mut self.composer_history_stash);
                self.composer.update(cx, |input, cx| input.replace_all(stash, window, cx));
                self.move_composer_caret_to_end(window, cx);
                // Programmatic writes emit no synchronous signal, so refresh
                // suggestions explicitly for a recalled `@…` trigger.
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
        // Programmatic writes emit no synchronous signal, so refresh
        // suggestions explicitly for a recalled `@…` trigger.
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

/// History rows show the truncated first-line title but match against the full
/// prompt: Kit filters label + keywords by substring, so without the keyword
/// multi-line bodies and text past the 72-char title cut are unfindable.
fn history_commands(prompts: Vec<String>) -> Vec<PaletteCommand> {
    prompts
        .into_iter()
        .map(|prompt| {
            let mut entry =
                command(session_title_from_prompt(&prompt), PaletteAction::InsertHistoryPrompt(prompt.clone()));
            entry.shared_keywords.push(prompt.into());
            entry
        })
        .collect()
}
