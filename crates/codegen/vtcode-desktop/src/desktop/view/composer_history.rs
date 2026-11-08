use super::*;
use vtcode_commons::prompt_history::{MAX_PROMPT_HISTORY_ROWS, prompt_history_row_label, rank_prompt_history};

impl DesktopView {
    /// Open the history palette for Ctrl+R.
    ///
    /// Pressing Ctrl+R again must *not* toggle the palette closed: the CLI
    /// cycles to the next match on the same chord, so an already-open history
    /// palette cycles here too (`on_prompt_history_shortcut`). Only the
    /// trigger buttons use the toggle-close convention.
    ///
    /// Reverse search is inline: the panel shows rows only (no search field),
    /// and the composer draft is the live filter — every draft change
    /// re-ranks through `update_composer_suggestions`. The seed below only
    /// filters the first paint; cancel restores the pre-search draft.
    pub(super) fn open_prompt_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::PromptHistory {
            return;
        }
        // Stash the exact pre-search draft (untrimmed): the filter is typed
        // into the composer, so cancel must restore this verbatim.
        let draft = self.composer.read(cx).value();
        self.history_filter_stash = draft.clone();
        let seed = draft.trim().to_string();
        self.show_palette(PaletteKind::PromptHistory, self.history_palette_groups(&seed), window, cx);
        // `show_palette` clears the query: mirror the untrimmed draft (the
        // ranker trims internally) so the unchanged-text guard matches first try.
        self.palette_query = draft;
        self.request_recent_prompts();
    }

    /// Ctrl+R while the history palette is open: open it first, otherwise
    /// cycle to the next match (CLI `KeyCode::Char('r') if CONTROL` parity).
    pub(super) fn on_prompt_history_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::PromptHistory {
            if !self.move_composer_suggestion(true, window, cx) {
                // A query matching no rows has nothing to cycle: stay open so
                // the user can edit the filter instead of losing the palette.
                cx.notify();
            }
            return;
        }
        self.open_prompt_history(window, cx);
    }

    /// Ctrl+G while the history palette is open: cancel and restore the draft
    /// (CLI parity). Finished elsewhere by the palette `on_cancel` path so
    /// focus returns through the same code as Escape.
    pub(super) fn cancel_prompt_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() && self.palette_kind == PaletteKind::PromptHistory {
            self.close_palette(window, cx);
        }
    }

    /// Whether the history palette has no prompts at all, ignoring the query.
    ///
    /// The palette filters host-side, so an empty row list cannot tell "no
    /// history yet" from "nothing matched this query"; the empty-state copy
    /// needs the unfiltered answer.
    pub(super) fn prompt_history_is_empty(&self) -> bool {
        self.prompt_history_entries().is_empty()
            && (self.history_scope == HistoryScope::Session
                || self.recent_prompts_scope != Some(self.history_scope)
                || self.recent_prompts.is_empty())
    }

    /// Re-rank the history palette for `query`, driven from the composer
    /// draft on every change. The palette runs with Kit filtering disabled,
    /// so these rows are exactly what the user sees; matching, ranking,
    /// dedup, and capping all come from the shared CLI ranker. Re-ranking
    /// resets the highlight like a fresh query (Kit did this while it owned
    /// the input): first row when rows exist, none when empty.
    pub(super) fn refresh_prompt_history(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_kind != PaletteKind::PromptHistory || self.palette.is_none() {
            return;
        }
        self.palette_query = query.into();
        let groups = self.history_palette_groups(query);
        let has_rows = groups.iter().any(|(_, entries)| !entries.is_empty());
        self.palette_groups = share_palette_commands(groups);
        if has_rows && let Some(state) = self.palette.clone() {
            if state.read(cx).matched_count() > 0 {
                // Fresh model: resolve the highlight now so Enter accepts the
                // first row without waiting a frame.
                let first = Some(gpui_kit::component::IndexPath::new(0));
                state.update(cx, |state, cx| state.set_selected_index(first, window, cx));
            } else {
                // Stale-empty model (the filter went 0→N): the new rows
                // install at render, so resolve the highlight after that.
                cx.on_next_frame(window, |view, window, cx| view.select_first_history_match(window, cx));
            }
        }
        cx.notify();
    }

    /// Post-install highlight for history re-ranks that landed while Kit's
    /// model was stale-empty. Skips when something is already highlighted —
    /// a newer refresh or arrow navigation may have won the race.
    pub(super) fn select_first_history_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_kind != PaletteKind::PromptHistory || self.palette.is_none() {
            return;
        }
        if !self.palette_groups.iter().any(|(_, entries)| !entries.is_empty()) {
            return;
        }
        if let Some(state) = self.palette.clone() {
            let highlighted = state.read(cx).selected_index().is_some();
            if !highlighted {
                state.update(cx, |state, cx| {
                    state.set_selected_index(Some(gpui_kit::component::IndexPath::new(0)), window, cx)
                });
            }
        }
    }

    /// Palette groups for Ctrl+R: the live thread first, then cached
    /// cross-thread recents for the current scope.
    ///
    /// Each source is ranked by the shared CLI ranker (nucleo fuzzy, scored,
    /// deduplicated, capped at [`MAX_PROMPT_HISTORY_ROWS`]); duplicates keep
    /// the live entry because it is ranked first. The two sections are kept
    /// because they name the prompt's origin — the recents section would be
    /// indistinguishable from this thread otherwise. Session scope shows the
    /// live thread only; other scopes add recents so brand-new sessions offer
    /// history search instead of an empty palette.
    fn history_palette_groups(&self, query: &str) -> Vec<(String, Vec<PaletteCommand>)> {
        let thread_prompts = self.prompt_history_entries();
        let thread_candidates: Vec<&str> = thread_prompts.iter().map(String::as_str).collect();
        let thread = ranked_prompt_rows(query, &thread_candidates);

        let mut others = Vec::new();
        if self.history_scope != HistoryScope::Session && self.recent_prompts_scope == Some(self.history_scope) {
            // Dedup against every live prompt rather than only the ranked
            // survivors: a prompt this thread already owns must not reappear
            // as a recents row just because the query hid it above.
            let in_thread: HashSet<&str> = thread_candidates.iter().copied().collect();
            let recent_candidates: Vec<&str> = self
                .recent_prompts
                .iter()
                .map(|recent| recent.content.as_str())
                .filter(|content| !content.trim().is_empty() && !in_thread.contains(content))
                .collect();
            // One merged cap across both sections, like the CLI's single
            // ranked list: ranking each section to the full limit would let a
            // long live thread plus recents render up to twice the budget.
            let remaining = MAX_PROMPT_HISTORY_ROWS.saturating_sub(thread.len());
            others = ranked_prompt_rows(query, &recent_candidates);
            others.truncate(remaining);
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
            // Re-rank with the live query so a scope switch or a late recents
            // response keeps the current filter instead of resetting it.
            let query = self.palette_query.to_string();
            self.palette_groups = share_palette_commands(self.history_palette_groups(&query));
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

/// One ranked prompt row: the prompt text plus its `· +N lines` display label.
struct RankedPromptRow {
    prompt: String,
    label: String,
}

/// Rank `candidates` with the shared CLI ranker and build display rows.
///
/// The palette runs with Kit filtering disabled, so no keyword mirroring is
/// needed here: matching already happened before the rows were built.
fn ranked_prompt_rows(query: &str, candidates: &[&str]) -> Vec<RankedPromptRow> {
    rank_prompt_history(query, candidates, MAX_PROMPT_HISTORY_ROWS)
        .into_iter()
        .filter_map(|index| candidates.get(index).copied())
        .map(|prompt| RankedPromptRow {
            prompt: prompt.to_owned(),
            label: prompt_history_row_label(prompt, ""),
        })
        .collect()
}

/// History rows render the collapsed first-line label; accepting the row
/// restores the full prompt.
fn history_commands(rows: Vec<RankedPromptRow>) -> Vec<PaletteCommand> {
    rows.into_iter()
        .map(|row| command(row.label, PaletteAction::InsertHistoryPrompt(row.prompt)))
        .collect()
}
