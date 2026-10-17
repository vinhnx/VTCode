---
feature: tui-config-search
status: delivered
updated: 2026-09-26
branch: compose/tui-config-search
commits: cbcdb8e15..97fcfeacd
---

# Responsive TUI Configuration Search

## Report

**What was built** — TUI pickers filter options live as the user types. Matching is centralized in `ListSearchFilter`
(exact terms or nucleo fuzzy) over a lowercased haystack of title + description + keywords.

The shared list modal (`/config`, overlays) delegates to that filter: `search_value` remains the sole corpus when set,
otherwise title + subtitle is used. The standalone ratatui picker (`run_interactive_selection` for
model/reasoning/tier/first-run) gained a search row (auto-enabled at ≥3 entries), filtered navigation, Esc
clear-then-cancel, digit-jump only while the query is empty, and Enter that returns the original catalog index.
`SelectionEntry::keywords` feeds provider labels and model ids into the haystack.

**Verification** — commands run and observed results:

- `./scripts/check-dev.sh` — PASS
- `cargo nextest run -p vtcode-ui` — PASS (1427)
- `cargo nextest run -p vtcode -E 'test(interactive) or test(model_picker) or test(first_run) or test(search) or test(settings)'`
  — PASS (235)
- New tests: `ListSearchFilter` (5), modal `apply_search` fallback/override (2), interactive_list state + `handle_event`
  (including j/k, digits, Esc, Enter-block, page clamp) — PASS
- PRE-EXISTING: `prompts::system::tests::test_golden_multi_section_output_is_byte_identical` (env-dependent skills list;
  `system.rs` untouched)

Independent review found j/k stolen by typing and PageDown wrap; both fixed and re-reviewed (no remaining criticals).
Documented tradeoff: `j` cannot start a filter while the query is empty.

**Journey log** —

1. Fuzzy subsequence is generous — `gpt` matches `…reasoning tools anthropic claude-sonnet-4`. Tests should use unique
   tokens (`zhipu`) rather than short ambiguous queries.
2. Preserve advertised vim keys (`j`/`k`) when adding a type-to-filter field: route printables to the query only once
   filtering is active, or the hint lies.
3. Page jumps must clamp; wrap is only correct for single-step arrows.
4. One haystack builder (`SearchCandidate::haystack`) for production and tests avoids drifting corpora.

## [S1] Problem

TUI configuration pickers must filter options as the user types, matching labels, descriptions, and keywords. The shared
list modal already has live search (`ModalSearchState` + `FuzzyQuery`), and `/config` already indexes
path/label/value/description/options into `InlineListItem::search_value`. The remaining gap is the standalone ratatui
picker `run_interactive_selection` (model, reasoning effort, service tier, first-run prompts): it has keyboard
navigation and digit shortcuts but **no search field**, so long option lists cannot be filtered. Search matching also
lives in two layers (`ModalListState::apply_search` vs nothing in `interactive_list`), so behavior and keywords are not
reusable.

The request is to extend the shared common modal stack with a **reusable search text field** and use it for TUI
configuration options, while preserving keyboard navigation, focus, scrolling, selection, existing modal behavior, and
test coverage.

## [S2] Design

### Decisions

1. **One matching engine** — reuse `vtcode_ui::tui::ui::search::{normalize_query, FuzzyQuery, exact_terms_match}`. No
   second matcher in `interactive_list`.
2. **Reusable filter state** — extract a small `ListSearchFilter` (query + fuzzy + matching over
   titles/descriptions/keywords) used by both `ModalListState::apply_search` and `interactive_list`.
3. **`SelectionEntry` search corpus** — each entry matches on `title` + `description` (when present) + optional
   `keywords` (new field, default empty). Config option builders already put those strings in `search_value`;
   interactive_list gets the same via the new `keywords`.
4. **interactive_list gains a search field** — always visible when `entries.len() > SEARCH_MIN_ENTRIES` (default 3) or
   the caller opts in. Typing filters live; visible list shrinks; selection index is remapped to the filtered list.
5. **Preserve existing modal behavior** — shared modal search path (typing, backspace, Esc-to-clear, fuzzy ranking,
   header groups, selection memory, hit-test rows) is unchanged in contract; only call-site plumbing moves to the shared
   filter helper.

### Reusable search filter

In `crates/codegen/vtcode-ui/src/tui/ui/search.rs` (or a sibling `list_filter.rs` under `tui/ui/`):

```rust
/// Compiled query over a list of candidates. One query, many scores.
pub struct ListSearchFilter {
    query: String,
    fuzzy: bool,
}

pub struct SearchCandidate<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub keywords: &'a [String], // or a single &str haystack
}

impl ListSearchFilter {
    pub fn new(query: &str, fuzzy: bool) -> Self;
    pub fn is_active(&self) -> bool;
    /// Haystack: title + description + keywords, lowercased via normalize_query.
    pub fn matches(&mut self, candidate: &SearchCandidate<'_>) -> bool;
    /// Indices of matching candidates in original order (stable) or ranked
    /// when `fuzzy` (modal keeps its existing ranked-block behavior).
    pub fn filter_indices(&mut self, candidates: &[SearchCandidate<'_>]) -> Vec<usize>;
}
```

Matching rules (shared):

| Mode                   | Rule                                                                                       |
| ---------------------- | ------------------------------------------------------------------------------------------ |
| empty query            | match all                                                                                  |
| exact (`fuzzy: false`) | every whitespace-separated query term is a substring of the haystack (`exact_terms_match`) |
| fuzzy (`fuzzy: true`)  | nucleo subsequence score against the haystack (`FuzzyQuery::score`)                        |

Haystack construction is identical in both surfaces: `title` + optional `description` + keyword blob. Modal
`search_value` remains the caller-supplied haystack override when present (keeps `/config` path/label/value indexing).

### interactive_list contract

`run_interactive_selection(title, instructions, entries, default_index)` stays the public entry. Behavior:

1. Render a search input row under `instructions` when search is enabled for the entry count.
2. **Typing** (printable chars) appends to the query and refilters `visible: Vec<usize>` (original indices). Matching
   uses `title` + `description` + `keywords`.
3. **Backspace** deletes one char; **Esc** clears the query when non-empty (same as modal) and only cancels the picker
   when the query is already empty.
4. **↑/↓/k/j/Home/End/PageUp/PageDown** move selection within `visible` only. Empty filter result shows a single dimmed
   "No matching options" row; Enter does nothing; Esc clears the query.
5. **Enter/Tab** selects the currently highlighted **visible** entry and returns its **original** index
   (`visible[selected]`).
6. **Digit shortcuts** apply only when the query is empty (existing behavior). When the query is non-empty, digits type
   into the search field (consistent with modal `digit_with_open_search_filters_instead_of_selecting`).
7. **Ctrl+C** still raises `SelectionInterrupted`.
8. Scrolling uses `ListState` over `visible`; selection clamps when the filter shrinks (keep first visible item or
   nearest previous match).

`SelectionEntry` gains:

```rust
pub struct SelectionEntry {
    pub title: String,
    pub description: Option<String>,
    pub keywords: Vec<String>, // default empty; labels/aliases/ids
}
```

`SelectionEntry::new` keeps its signature (keywords empty); add `with_keywords(...)` builder. Callers that already build
long description strings may also pass provider/model id strings as keywords.

### Shared modal contract (unchanged surface)

- `ModalSearchState` / `InlineListSearchConfig` stay the wire API.
- `ModalListState::apply_search` delegates term matching to `ListSearchFilter` for the per-item haystack (`search_value`
  or title+subtitle); ranked fuzzy header-block grouping stays in `ModalListState` (already tested).
- Search row count (`summary_line_rows`, hit-test, status strip) is unchanged.
- `/config` `settings_search_config` remains fuzzy with view-specific placeholders.

### Error / empty behavior

- No matches: modal keeps `No matching options` + `Press Esc to reset`; interactive_list shows `No matching options` and
  blocks Enter.
- Focus: the search field is implicit (always receiving printable input) in both surfaces — no Tab-to-focus required,
  matching today's modal.

### Tests

- `ListSearchFilter`: empty query, multi-term exact, fuzzy subsequence, keywords-only match, description-only match.
- `interactive_list`: typing filters and remaps Enter to the original index; Esc clears then cancels; digits type when
  query non-empty; navigation stays on visible subset; empty result blocks selection.
- Shared modal regression: existing fuzzy/header/selection tests keep passing; one test that `apply_search` still honors
  `search_value` overrides.
- Config option builders: at least one test that a settings row's `search_value` contains label + description terms
  (already partially covered).

## [S3] Out of Scope

- Changing `/config` item hierarchy, tones, or status strips from `config-models-modal-ux`.
- Mouse hit-testing for interactive_list.
- Non-TUI / plain CLI fallbacks beyond keeping them compiling.
- New configuration schema fields.
- Fuzzy ranking changes in the modal header-block algorithm (keep as-is).

## Tasks

- [x] T1: Shared `ListSearchFilter` over title/description/keywords — acceptance: unit tests cover
      empty/exact/fuzzy/keywords/description matching; used by one production caller (covers: S2)
- [x] T2: Wire shared filter into modal `apply_search` without behavior change — acceptance: existing modal search tests
      pass unchanged; one test pins `search_value` override (covers: S2; depends: T1)
- [x] T3: interactive_list live search field + filtered navigation/selection — acceptance: typing filters options; Enter
      returns original index; Esc clear-then-cancel; digits type when query non-empty; empty result blocks Enter
      (covers: S2; depends: T1)
- [x] T4: `SelectionEntry` keywords + call-site haystacks for model/reasoning/tier/first-run — acceptance: model id /
      provider label searchable via keywords on the TUI picker (covers: S2; depends: T3)
- [x] T5: Regression suite + docs note — acceptance: `./scripts/check-dev.sh` green; focused nextest for
      search/modal/interactive_list green; short note in `docs/development/` on TUI list search (covers: S2)
